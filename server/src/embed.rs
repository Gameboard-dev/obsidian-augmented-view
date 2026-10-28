//! The sentence-embedding model (BAAI/bge-base-en-v1.5, <https://huggingface.co/BAAI/bge-base-en-v1.5>) on the GPU in
//! float32 (with TF32 matrix products on CUDA) with candle (<https://docs.rs/candle-transformers/latest/candle_transformers/models/bert/>): the [CLS] vector,
//! normalised, as the model card prescribes; on the CPU when there is no GPU for it (hw.rs).

use candle_core::{DType, Device, Result, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config};
use tokenizers::{PaddingParams, Tokenizer, TruncationParams};

use crate::hw;
use crate::models::need;

pub struct Embedder {
    model: BertModel,
    tok: Tokenizer,
    device: Device,
}

impl Embedder {
    pub fn load(repo: &str, device: &Device) -> Result<Embedder> {
        let config: Config = serde_json::from_str(&std::fs::read_to_string(need(repo, "config.json")?)?).map_err(candle_core::Error::wrap)?;
        // float32: candle's BERT always builds its attention mask as (1 - mask) x f32::MIN, and f32::MIN is -inf in fp16
        // and bf16, so even an unpadded batch gives 0 x -inf = NaN. TF32 lets float32 matrix products use the tensor cores
        // (<https://docs.rs/candle-core/latest/candle_core/cuda_backend/fn.set_gemm_reduced_precision_f32.html>).
        #[cfg(feature = "cuda")]
        candle_core::cuda::set_gemm_reduced_precision_f32(true);
        let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[need(repo, "model.safetensors")?], DType::F32, device)? };
        let mut tok = Tokenizer::from_file(need(repo, "tokenizer.json")?).map_err(candle_core::Error::wrap)?;
        tok.with_padding(Some(PaddingParams::default()));
        tok.with_truncation(Some(TruncationParams { max_length: 512, ..Default::default() })).map_err(candle_core::Error::wrap)?;
        Ok(Embedder { model: BertModel::load(vb, &config)?, tok, device: device.clone() })
    }

    /// Unit vectors, one per text of each group, in order. On the GPU all in one padded batch (measured faster than one
    /// batch per text length); on the CPU, where every padded token costs, one batch per group (short phrases are then
    /// not padded to a sentence's length).
    pub fn encode_groups(&self, groups: &[&[&str]]) -> Result<Vec<Vec<f32>>> {
        if !self.device.is_cpu() {
            return self.encode(&groups.concat());
        }
        let mut out = Vec::new();
        for g in groups.iter().filter(|g| !g.is_empty()) {
            out.extend(self.encode(g)?);
        }
        Ok(out)
    }

    /// Unit vectors, one per text, in one padded batch.
    pub fn encode(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let enc = self.tok.encode_batch(texts.to_vec(), true).map_err(candle_core::Error::wrap)?;
        let rows = |f: &dyn Fn(&tokenizers::Encoding) -> &[u32]| -> Result<Tensor> {
            Tensor::new(enc.iter().map(|e| f(e).to_vec()).collect::<Vec<_>>(), &self.device)
        };
        let (ids, mask) = (rows(&|e| e.get_ids())?, rows(&|e| e.get_attention_mask())?);
        hw::paced(&self.device, || {
            let cls = self.model.forward(&ids, &ids.zeros_like()?, Some(&mask))?.narrow(1, 0, 1)?.squeeze(1)?;
            cls.broadcast_div(&cls.sqr()?.sum_keepdim(1)?.sqrt()?)?.to_vec2()
        })
    }
}
