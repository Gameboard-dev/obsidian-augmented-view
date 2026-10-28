//! Sentence colours and contrast words, from Qwen3-4B-Instruct-2507 (<https://huggingface.co/Qwen/Qwen3-4B-Instruct-2507>)
//! run by candle's quantized Qwen3 (<https://docs.rs/candle-transformers/latest/candle_transformers/models/quantized_qwen3/>).
//! (The module keeps its first job's name: it ranked a paragraph's main point and critical points until 2026-10-25.)
//!
//! candle loads quantized weights only from GGUF (<https://github.com/ggml-org/ggml/blob/master/docs/gguf.md>), and Qwen
//! publishes none for this model, so on first use the official weights are quantized (Q4K; the tied embedding, which
//! also serves as the output layer, Q6K) and written once to a GGUF file in the cache folder (the official weights, if
//! this server downloaded them for it, are then deleted: 8 GB the server no longer needs).
//!
//! A paragraph's sentences share the prompt up to the question, so on the GPU that part runs once and its attention
//! cache is reused for each sentence ([`Claims::ask`]).

use std::{fs::File, io::BufWriter, path::Path};

use candle_core::quantized::{gguf_file, GgmlDType, QTensor};
use candle_core::{safetensors::MmapedSafetensors, DType, Device, Result, Tensor};
use candle_transformers::models::quantized_qwen3::ModelWeights;
use serde_json::{json, Value};
use tokenizers::Tokenizer;

use crate::hw;
use crate::models::{need, ours};
use crate::text::Doc;

const LETTERS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ"; // one token each, so one forward pass scores every clause
/// Contrast words: those a careful reader would stress. Generated (a short list), then kept only where they occur.
const STRESS_Q: &str = "Which single words in this paragraph would a careful reader stress because the paragraph's meaning turns on \
    them: a word that sets one thing apart from another (an adjective such as external, extra or ordinary), an exception or limit \
    (only, none, not, except, outside, including), or a contrast (instead, unlike, rather)? Do not list topic nouns. Give at most \
    six words, exactly as they appear, most important first, one per line, and nothing else; if there are none, answer NONE.";
const STRESS_MAX_TOKENS: usize = 40;
const STRESS_MIN_WORDS: usize = 12; // a shorter paragraph has nothing to contrast
const CONTEXT: u32 = 8192; // tokens; a prompt of 26 long clauses stays well within it
const COLOUR_SYSTEM: &str = "You mark up academic writing for a reader who skims. Each category is a colour that shows the role a \
    sentence plays in its paragraph. Judge the sentence's own role, in the context of the paragraph.";
const COLOUR_Q: &str = "Which category best describes the role of this sentence? Answer with its letter only.";
const COLOUR_NONE: &str = "The sentence plays none of the roles above";
const COLOUR_MIN_WORDS: usize = 3; // a shorter "sentence" is a fragment of markup

/// A colour the model looks for (docs/colour-codes.md): its index in the colour code, its name and its description.
#[derive(serde::Deserialize, Clone)]
pub struct Category {
    pub i: usize,
    pub name: String,
    #[serde(default)]
    pub description: String,
}

pub struct Claims {
    model: ModelWeights,
    tok: Tokenizer,
    letters: Vec<u32>,
    device: Device,
    /// What the answer starts with: for a Qwen3 model that can think (its chat template takes `enable_thinking`), the
    /// empty thinking block its template writes when thinking is off, so the first answer token is the answer
    /// (<https://huggingface.co/Qwen/Qwen3-8B#switching-between-thinking-and-non-thinking-mode>).
    reply: &'static str,
    /// GLiNER2 for the phrase colours, loaded on first use (Err: why it could not be, said once).
    phrases: Option<std::result::Result<crate::phrases::Phrases, String>>,
}

/// Quantize the official weights into a GGUF file candle's quantized Qwen3 reads (its names and metadata keys).
fn convert(repo: &str, out: &Path) -> Result<()> {
    let cfg: Value = serde_json::from_str(&std::fs::read_to_string(need(repo, "config.json")?)?).map_err(candle_core::Error::wrap)?;
    let index: Value = serde_json::from_str(&std::fs::read_to_string(need(repo, "model.safetensors.index.json")?)?).map_err(candle_core::Error::wrap)?;
    let mut shards: Vec<String> = index["weight_map"].as_object().into_iter().flatten().filter_map(|(_, f)| f.as_str().map(String::from)).collect();
    shards.sort();
    shards.dedup();
    let paths = shards.iter().map(|f| need(repo, f)).collect::<Result<Vec<_>>>()?;
    let st = unsafe { MmapedSafetensors::multi(&paths)? };
    let rename = |n: &str| -> Option<String> {
        let n = n.strip_prefix("model.").unwrap_or(n);
        let fixed = [("embed_tokens.weight", "token_embd.weight"), ("norm.weight", "output_norm.weight"), ("lm_head.weight", "output.weight")];
        if let Some((_, g)) = fixed.iter().find(|(h, _)| *h == n || format!("model.{h}") == n) {
            return Some(g.to_string());
        }
        let (layer, rest) = n.strip_prefix("layers.")?.split_once('.')?;
        let part = [("input_layernorm", "attn_norm"), ("post_attention_layernorm", "ffn_norm"), ("self_attn.q_proj", "attn_q"), ("self_attn.k_proj", "attn_k"),
            ("self_attn.v_proj", "attn_v"), ("self_attn.o_proj", "attn_output"), ("self_attn.q_norm", "attn_q_norm"), ("self_attn.k_norm", "attn_k_norm"),
            ("mlp.gate_proj", "ffn_gate"), ("mlp.up_proj", "ffn_up"), ("mlp.down_proj", "ffn_down")]
            .iter().find(|(h, _)| rest == format!("{h}.weight"))?.1;
        Some(format!("blk.{layer}.{part}.weight"))
    };
    let mut tensors: Vec<(String, QTensor)> = Vec::new();
    for (name, _) in st.tensors() {
        let Some(gguf) = rename(&name) else { continue };
        let t = st.load(&name, &Device::Cpu)?.to_dtype(DType::F32)?;
        let dtype = if t.rank() == 1 { GgmlDType::F32 } else if gguf == "token_embd.weight" { GgmlDType::Q6K } else { GgmlDType::Q4K };
        eprintln!("quantizing {gguf} {:?} as {dtype:?}", t.dims());
        tensors.push((gguf, QTensor::quantize(&t, dtype)?));
    }
    let u = |k: &str| gguf_file::Value::U32(cfg[k].as_u64().unwrap_or(0) as u32);
    let f = |k: &str| gguf_file::Value::F32(cfg[k].as_f64().unwrap_or(0.0) as f32);
    let meta = [
        ("general.architecture", gguf_file::Value::String("qwen3".into())),
        ("general.dtype", gguf_file::Value::U32(1)), // activations in f16
        ("qwen3.attention.head_count", u("num_attention_heads")),
        ("qwen3.attention.head_count_kv", u("num_key_value_heads")),
        ("qwen3.attention.key_length", u("head_dim")),
        ("qwen3.block_count", u("num_hidden_layers")),
        ("qwen3.embedding_length", u("hidden_size")),
        ("qwen3.context_length", gguf_file::Value::U32(CONTEXT)),
        ("qwen3.attention.layer_norm_rms_epsilon", f("rms_norm_eps")),
        ("qwen3.rope.freq_base", f("rope_theta")),
    ];
    let tmp = out.with_extension("part");
    let mut w = BufWriter::new(File::create(&tmp)?);
    gguf_file::write(&mut w, &meta.iter().map(|(k, v)| (*k, v)).collect::<Vec<_>>(), &tensors.iter().map(|(k, v)| (k.as_str(), v)).collect::<Vec<_>>())?;
    drop(w);
    std::fs::rename(tmp, out)?;
    drop(st);
    for (p, _) in paths.iter().zip(&shards).filter(|(p, f)| **p == ours(repo, f)) {
        _ = std::fs::remove_file(p); // only shards this server downloaded, never the Hugging Face cache's
    }
    Ok(())
}

impl Claims {
    pub fn load(repo: &str, cache: &Path, device: &Device) -> Result<Claims> {
        let gguf = cache.join(format!("{}-q4k.gguf", repo.replace('/', "--")));
        if !gguf.exists() {
            eprintln!("converting {repo} to {} (once)", gguf.display());
            convert(repo, &gguf)?;
        }
        let mut file = File::open(&gguf)?;
        let content = gguf_file::Content::read(&mut file)?;
        let model = ModelWeights::from_gguf(content, &mut file, device)?;
        let tok = Tokenizer::from_file(need(repo, "tokenizer.json")?).map_err(candle_core::Error::wrap)?;
        let letters = LETTERS.chars().map(|c| tok.token_to_id(&c.to_string()).ok_or_else(|| candle_core::Error::Msg(format!("no token for {c}")))).collect::<Result<_>>()?;
        let template = std::fs::read_to_string(need(repo, "tokenizer_config.json")?).unwrap_or_default();
        let reply = if template.contains("enable_thinking") { "<think>\n\n</think>\n\n" } else { "" };
        Ok(Claims { model, tok, letters, device: device.clone(), reply, phrases: None })
    }

    /// The log-probabilities of the first `n` letters as the answer to `prefix` followed by each question. On the GPU
    /// the prefix runs once and each question continues from a copy of its attention cache (candle's GPU cache appends
    /// new tensors, so a copy shares the prefix's without changing it); on the CPU, where the cache is written in place,
    /// or where the tokens at the join differ from the whole prompt's, each prompt runs whole.
    fn ask(&mut self, prefix: &str, questions: &[String], n: usize) -> Result<Vec<Vec<f32>>> {
        let encode = |s: &str| self.tok.encode(s, false).map(|e| e.get_ids().to_vec()).map_err(candle_core::Error::wrap);
        let pre = encode(prefix)?;
        let (dev, letters) = (self.device.clone(), Tensor::new(&self.letters[..n], &self.device)?);
        let ids = |t: &[u32]| Tensor::new(t, &dev)?.unsqueeze(0);
        let mut primed = false;
        let mut out = Vec::new();
        for q in questions {
            let full = encode(&format!("{prefix}{q}"))?;
            let model = &mut self.model;
            let l = hw::paced(&dev, || -> Result<Vec<f32>> {
                let logits = if !dev.is_cpu() && full.len() > pre.len() && full.starts_with(&pre) {
                    if !primed {
                        model.clear_kv_cache();
                        model.forward(&ids(&pre)?, 0)?;
                        primed = true;
                    }
                    model.clone().forward(&ids(&full[pre.len()..])?, pre.len())?
                } else {
                    model.clear_kv_cache();
                    primed = false;
                    model.forward(&ids(&full)?, 0)?
                };
                logits.squeeze(0)?.to_dtype(DType::F32)?.index_select(&letters, 0)?.to_vec1::<f32>()
            })?;
            let max = l.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            let lse = max + l.iter().map(|x| (x - max).exp()).sum::<f32>().ln();
            out.push(l.iter().map(|x| x - lse).collect());
        }
        Ok(out)
    }

    /// Greedy decoding from `prompt` until the end of the turn or `max` tokens (Qwen's chat format,
    /// <https://qwen.readthedocs.io/en/latest/getting_started/concepts.html#chat-template>).
    fn generate(&mut self, prompt: &str, max: usize) -> Result<String> {
        let ids = self.tok.encode(prompt, false).map_err(candle_core::Error::wrap)?.get_ids().to_vec();
        let (dev, end) = (self.device.clone(), self.tok.token_to_id("<|im_end|>"));
        let model = &mut self.model;
        let out = hw::paced(&dev, || -> Result<Vec<u32>> {
            model.clear_kv_cache();
            let mut logits = model.forward(&Tensor::new(ids.as_slice(), &dev)?.unsqueeze(0)?, 0)?;
            let (mut out, mut pos) = (Vec::new(), ids.len());
            for _ in 0..max {
                let next = logits.squeeze(0)?.to_dtype(DType::F32)?.argmax(0)?.to_scalar::<u32>()?;
                if Some(next) == end {
                    break;
                }
                out.push(next);
                logits = model.forward(&Tensor::new(&[next], &dev)?.unsqueeze(0)?, pos)?;
                pos += 1;
            }
            Ok(out)
        })?;
        self.tok.decode(&out, true).map_err(candle_core::Error::wrap)
    }

    /// Contrast words: the model's list, each kept where it occurs in the paragraph as a word of its own (outside
    /// links, citations and code), in the model's order; "w" is the place in that order.
    fn stress(&mut self, d: &Doc) -> Result<Vec<Value>> {
        if d.toks.iter().filter(|t| t.alpha()).count() < STRESS_MIN_WORDS {
            return Ok(Vec::new());
        }
        let prompt = format!("<|im_start|>user\nParagraph:\n{}\n\n{STRESS_Q}<|im_end|>\n<|im_start|>assistant\n{}", d.visible(0, d.text.len()), self.reply);
        let answer = self.generate(&prompt, STRESS_MAX_TOKENS)?;
        let mut out: Vec<Value> = Vec::new();
        for line in answer.lines().map(str::trim).filter(|l| !l.is_empty() && *l != "NONE") {
            let w = line.trim_start_matches(|c: char| c == '-' || c == '*' || c == '.' || c.is_ascii_digit()).trim().trim_matches(['"', '\'', '*']).to_lowercase();
            if w.is_empty() || w.contains(' ') || out.iter().any(|o| o["v"] == w) {
                continue;
            }
            // every use of the word but those in a fixed phrase, where nothing turns on it ("whether or not it says so")
            let idiom = |t: usize| w == "not" && t > 0 && d.toks[t - 1].lower == "or";
            let found: Vec<usize> = (0..d.toks.len()).filter(|&t| d.toks[t].lower == w && !idiom(t) && !d.in_link(t) && d.clean(d.toks[t].s, d.toks[t].e)).collect();
            let Some(&first) = found.first() else { continue }; // a word the model made up, or one only in a link
            // the whole written word when the parser split it: "can|not" is drawn as "cannot"
            let word = |t: usize| {
                let (mut a, mut b) = (t, t);
                while a > 0 && !d.space_after(a - 1) && d.toks[a - 1].alpha() {
                    a -= 1;
                }
                while b + 1 < d.toks.len() && !d.space_after(b) && d.toks[b + 1].alpha() {
                    b += 1;
                }
                (d.toks[a].s, d.toks[b].e)
            };
            let at: Vec<[u32; 2]> = found.iter().map(|&t| word(t)).map(|(s, e)| [d.utf16(s), d.utf16(e)]).collect();
            let (s, e) = word(first);
            out.push(json!({ "k": "stress", "t": &d.text[s..e], "w": out.len() as f32, "v": w, "at": at }));
        }
        Ok(out)
    }

    /// Each sentence's colour (docs/colour-codes.md, "The model's part"): the colours, lettered, with their descriptions,
    /// then the paragraph, then one question per sentence; the logits of the letters at the first answer position give
    /// the colour, so nothing is generated, and the listing up to the question runs once for all the sentences
    /// ([`Claims::ask`]). The last letter is "none of these"; a sentence it wins is left out. "w" is the chosen colour's
    /// probability, so the plugin can draw only the surer ones. Contrast words first when `stress`.
    ///
    /// Measured against variants that did no better (docs/models.md): the colours listed in two orders, calibration
    /// against an empty sentence, the sentence's place in the paragraph, a hint to prefer a sentence's job over its
    /// topic, the sentence marked inside the paragraph, the colours' names instead of letters, and a short reason
    /// generated before the answer (as good, 18 times slower).
    pub fn colour(&mut self, d: &Doc, colours: &[Category], phrases: &[Category], stress: bool) -> Result<Vec<Value>> {
        let mut out = if stress { self.stress(d)? } else { Vec::new() };
        if !phrases.is_empty() {
            let model = self.phrases.get_or_insert_with(|| crate::phrases::Phrases::load().map_err(|e| {
                if e.contains(crate::models::MISSING) {
                    hw::note(format!("phrase colours: {} (about 0.84 GB) is not downloaded: stop the server and start it again, and Settings → Augmented View → Models shows the command that downloads it", crate::phrases::REPO));
                } else {
                    hw::note(format!("phrase colours: GLiNER2 could not load: {e}"));
                }
                e
            }));
            if let Ok(model) = model {
                // a failure loses the phrases only, never the sentences' colours
                match model.find(d, phrases) {
                    Ok(found) => out.extend(found),
                    Err(e) => eprintln!("{e}"),
                }
            }
        }
        let sents: Vec<(usize, usize)> = d.sentence_spans().into_iter()
            .filter(|&(s, e)| d.toks.iter().filter(|t| t.s >= s && t.e <= e && t.alpha()).count() >= COLOUR_MIN_WORDS).collect();
        let n = colours.len() + 1;
        if sents.is_empty() || colours.is_empty() || n > LETTERS.len() {
            return Ok(out);
        }
        let listing: Vec<String> = colours.iter().map(|c| (c.name.as_str(), c.description.as_str())).chain([("None of these", COLOUR_NONE)]).enumerate()
            .map(|(j, (name, desc))| format!("{}. {name}: {}", &LETTERS[j..=j], desc.trim().trim_end_matches('.'))).collect();
        let prefix = format!("<|im_start|>system\n{COLOUR_SYSTEM}<|im_end|>\n<|im_start|>user\nCategories:\n{}\n\nParagraph:\n{}\n\n",
            listing.join("\n"), d.readable(0, d.text.len()));
        let questions: Vec<String> = sents.iter().map(|&(s, e)| format!("Sentence: {}\n\n{COLOUR_Q}<|im_end|>\n<|im_start|>assistant\n{}", d.readable(s, e), self.reply)).collect();
        for (&(s, e), lp) in sents.iter().zip(self.ask(&prefix, &questions, n)?) {
            let best = (0..n).max_by(|&a, &b| lp[a].total_cmp(&lp[b])).unwrap_or(n - 1);
            if best == n - 1 {
                continue;
            }
            let w = (lp[best].exp() * 100.0).round() / 100.0;
            out.push(json!({ "k": "colour", "c": colours[best].i, "t": &d.text[s..e], "w": w, "v": d.visible(s, e), "at": [[d.utf16(s), d.utf16(e)]] }));
        }
        Ok(out)
    }
}
