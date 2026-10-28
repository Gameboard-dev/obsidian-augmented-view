//! Phrase colours (COLOUR-CODES.md): spans of the paragraph found for each phrase colour's description by GLiNER2
//! (<https://github.com/fastino-ai/GLiNER2>; the checkpoint fastino/gliner2-base-v1, Apache-2.0),
//! run by the gliner2 crate (<https://github.com/paul-english/gliner2_rs>, candle, on the CPU: about 2.3 s a paragraph).
//! Measured against the instruct model choosing a colour for every noun phrase, it found as many of a reader's phrases
//! (63% against 60%) with nearly three times the precision (41% against 15%; README, "Colours"). Loaded the first time
//! a request asks for phrase colours, so a code without them costs nothing.

use gliner2::schema::{EntityTypeConfigInput, EntityTypesInput};
use gliner2::{CandleExtractor, ExtractOptions, ExtractorConfig, IndexMap, SchemaTransformer};
use serde_json::{json, Value};

use crate::claims::Category;
use crate::models::need;
use crate::text::Doc;

const REPO: &str = "fastino/gliner2-base-v1";
/// Spans below this confidence are not sent (the plugin's slider then sets its own floor above it): measured, 41% of
/// the spans at 0.3 or more are a reader's, 47% at 0.5, 51% at 0.7.
const FLOOR: f32 = 0.3;
/// Words read at a time: with the colours' descriptions they stay within the encoder's 512 tokens.
const CHUNK_WORDS: usize = 180;

pub struct Phrases {
    model: CandleExtractor,
    tokens: SchemaTransformer,
}

impl Phrases {
    pub fn load() -> Result<Phrases, String> {
        let e = |e: &dyn std::fmt::Display| e.to_string();
        let files = gliner2::config::ModelFiles {
            config: need(REPO, "config.json").map_err(|x| e(&x))?,
            encoder_config: need(REPO, "encoder_config/config.json").map_err(|x| e(&x))?,
            tokenizer: need(REPO, "tokenizer.json").map_err(|x| e(&x))?,
            weights: need(REPO, "model.safetensors").map_err(|x| e(&x))?,
        };
        let tokens = SchemaTransformer::new(&files.tokenizer.to_string_lossy()).map_err(|x| e(&x))?;
        let config: ExtractorConfig = serde_json::from_str(&std::fs::read_to_string(&files.config).map_err(|x| e(&x))?).map_err(|x| e(&x))?;
        let vocab = tokens.tokenizer.get_vocab_size(true);
        // on the CPU: the crate makes its inputs there (backends/candle/model.rs), so a model on the GPU would not run
        Ok(Phrases { model: CandleExtractor::load_cpu(&files, config, vocab).map_err(|x| e(&x))?, tokens })
    }

    /// The paragraph's spans for each colour, read in its masked text (markup blanked, links' words kept, the same
    /// offsets) a few sentences at a time (the encoder reads at most 512 tokens, the colours' descriptions included), as
    /// the protocol's spans: kind "colour", "c" the colour's place, "w" GLiNER2's confidence.
    pub fn find(&self, d: &Doc, colours: &[Category]) -> candle_core::Result<Vec<Value>> {
        let e = |e: &dyn std::fmt::Display| candle_core::Error::Msg(format!("GLiNER2: {e}"));
        let types: IndexMap<String, EntityTypeConfigInput> = colours.iter()
            .map(|c| (c.name.clone(), EntityTypeConfigInput::DescriptionOnly(c.description.clone()))).collect();
        let mut schema = gliner2::Schema::new();
        schema.entities(EntityTypesInput::WithMeta(types));
        let (schema, meta) = schema.build();
        let opts = ExtractOptions { threshold: FLOOR, include_confidence: true, include_spans: true, ..Default::default() };
        // runs of whole sentences of at most CHUNK_WORDS words (a longer sentence alone)
        let mut chunks: Vec<(usize, usize)> = Vec::new();
        let words = |s: usize, e: usize| d.masked[s..e].split_whitespace().count();
        for (s, e) in d.sentence_spans() {
            match chunks.last_mut() {
                Some(last) if words(last.0, e) <= CHUNK_WORDS => last.1 = e,
                _ => chunks.push((s, e)),
            }
        }
        let mut out = Vec::new();
        for (from, to) in chunks {
            // one chunk at a time: batching them was measured slower (padding)
            let text = &d.masked[from..to];
            let found = self.model.extract(&self.tokens, text, &schema, &meta, &opts).map_err(|x| e(&x))?;
            // character offsets of the chunk, as bytes of the paragraph
            let bytes: Vec<usize> = text.char_indices().map(|(b, _)| from + b).chain([to]).collect();
            let found = serde_json::to_value(&found).map_err(|x| e(&x))?; // {"entities": {label: [{"text", "confidence", "start", "end"}]}}
            for (label, hits) in found["entities"].as_object().into_iter().flatten() {
                let Some(c) = colours.iter().find(|c| &c.name == label) else { continue };
                for h in hits.as_array().into_iter().flatten() {
                    let (Some(s), Some(e)) = (h["start"].as_u64(), h["end"].as_u64()) else { continue };
                    let (Some(&s), Some(&e)) = (bytes.get(s as usize), bytes.get(e as usize)) else { continue };
                    let seg = &d.masked[s..e];
                    let (s, e) = (s + (seg.len() - seg.trim_start().len()), s + seg.trim_end().len());
                    if s >= e || !d.clean(s, e) {
                        continue;
                    }
                    let w = (h["confidence"].as_f64().unwrap_or(0.0) * 100.0).round() / 100.0;
                    out.push(json!({ "k": "colour", "c": c.i, "t": &d.text[s..e], "w": w, "v": d.visible(s, e), "at": [[d.utf16(s), d.utf16(e)]] }));
                }
            }
        }
        Ok(out)
    }
}
