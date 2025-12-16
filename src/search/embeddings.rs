//! Embedding model for semantic search using ONNX Runtime.
//!
//! Uses all-MiniLM-L6-v2 model for generating 384-dimensional embeddings.

use std::path::Path;
use std::sync::Mutex;

use ort::session::builder::GraphOptimizationLevel;
use ort::session::input::SessionInputValue;
use ort::session::Session;
use ort::value::Value;
use tokenizers::Tokenizer;

use crate::error::{Error, Result};

/// Model configuration
const MODEL_NAME: &str = "all-MiniLM-L6-v2";
const EMBEDDING_DIM: usize = 384;
const MAX_SEQ_LENGTH: usize = 256;

/// URLs for downloading model files from Hugging Face
const MODEL_URL: &str = "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/onnx/model.onnx";
const TOKENIZER_URL: &str = "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/tokenizer.json";

/// Global embedding model instance (loaded once)
static EMBEDDING_MODEL: Mutex<Option<EmbeddingModel>> = Mutex::new(None);

/// Embedding model wrapper
pub struct EmbeddingModel {
    session: Session,
    tokenizer: Tokenizer,
}

impl EmbeddingModel {
    /// Load the embedding model from disk, downloading if necessary
    pub fn load(models_dir: &Path) -> Result<Self> {
        let model_path = models_dir.join("model.onnx");
        let tokenizer_path = models_dir.join("tokenizer.json");

        // Download model files if they don't exist
        if !model_path.exists() || !tokenizer_path.exists() {
            tracing::info!("Downloading embedding model {}...", MODEL_NAME);
            Self::download_model_files(models_dir)?;
        }

        tracing::info!("Loading embedding model from {:?}", models_dir);

        // Load ONNX model
        let session = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_intra_threads(4)?
            .commit_from_file(&model_path)?;

        // Load tokenizer
        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| Error::Other(format!("Failed to load tokenizer: {}", e)))?;

        tracing::info!("Embedding model loaded successfully");
        Ok(Self { session, tokenizer })
    }

    /// Download model files from Hugging Face
    fn download_model_files(models_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(models_dir)?;

        let model_path = models_dir.join("model.onnx");
        let tokenizer_path = models_dir.join("tokenizer.json");

        // Download model.onnx
        if !model_path.exists() {
            tracing::info!("Downloading model.onnx...");
            Self::download_file(MODEL_URL, &model_path)?;
        }

        // Download tokenizer.json
        if !tokenizer_path.exists() {
            tracing::info!("Downloading tokenizer.json...");
            Self::download_file(TOKENIZER_URL, &tokenizer_path)?;
        }

        Ok(())
    }

    /// Download a file from URL to disk
    fn download_file(url: &str, dest: &Path) -> Result<()> {
        let response = ureq::get(url)
            .call()
            .map_err(|e| Error::Other(format!("Failed to download {}: {}", url, e)))?;

        let mut reader = response.into_reader();
        let mut file = std::fs::File::create(dest)?;
        std::io::copy(&mut reader, &mut file)?;

        Ok(())
    }

    /// Generate embedding for a single text
    pub fn embed(&mut self, text: &str) -> Result<Vec<f32>> {
        let embeddings = self.embed_batch(&[text])?;
        Ok(embeddings.into_iter().next().unwrap())
    }

    /// Generate embeddings for a batch of texts
    pub fn embed_batch(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        // Tokenize all texts
        let encodings = self
            .tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(|e| Error::Other(format!("Tokenization failed: {}", e)))?;

        let batch_size = encodings.len();

        // Prepare input tensors
        let mut input_ids: Vec<i64> = Vec::with_capacity(batch_size * MAX_SEQ_LENGTH);
        let mut attention_mask: Vec<i64> = Vec::with_capacity(batch_size * MAX_SEQ_LENGTH);
        let mut token_type_ids: Vec<i64> = Vec::with_capacity(batch_size * MAX_SEQ_LENGTH);

        for encoding in &encodings {
            let ids = encoding.get_ids();
            let mask = encoding.get_attention_mask();
            let types = encoding.get_type_ids();

            // Truncate or pad to MAX_SEQ_LENGTH
            let len = ids.len().min(MAX_SEQ_LENGTH);

            for i in 0..MAX_SEQ_LENGTH {
                if i < len {
                    input_ids.push(ids[i] as i64);
                    attention_mask.push(mask[i] as i64);
                    token_type_ids.push(types[i] as i64);
                } else {
                    input_ids.push(0);
                    attention_mask.push(0);
                    token_type_ids.push(0);
                }
            }
        }

        // Create input arrays
        let input_ids_array =
            ndarray::Array2::from_shape_vec((batch_size, MAX_SEQ_LENGTH), input_ids)
                .map_err(|e| Error::Other(format!("Failed to create input array: {}", e)))?;
        let attention_mask_array =
            ndarray::Array2::from_shape_vec((batch_size, MAX_SEQ_LENGTH), attention_mask)
                .map_err(|e| Error::Other(format!("Failed to create mask array: {}", e)))?;
        let token_type_ids_array =
            ndarray::Array2::from_shape_vec((batch_size, MAX_SEQ_LENGTH), token_type_ids)
                .map_err(|e| Error::Other(format!("Failed to create type array: {}", e)))?;

        // Create ORT values
        let input_ids_value = Value::from_array(input_ids_array)?;
        let attention_mask_value = Value::from_array(attention_mask_array)?;
        let token_type_ids_value = Value::from_array(token_type_ids_array)?;

        // Run inference
        let outputs = self.session.run(vec![
            ("input_ids", SessionInputValue::from(input_ids_value)),
            ("attention_mask", SessionInputValue::from(attention_mask_value)),
            ("token_type_ids", SessionInputValue::from(token_type_ids_value)),
        ])?;

        // Extract embeddings from output
        // The model outputs last_hidden_state with shape [batch_size, seq_len, hidden_size]
        // We use mean pooling over the sequence dimension
        let (shape, output_data) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| Error::Other(format!("Failed to extract output: {}", e)))?;

        // shape is [batch_size, seq_len, hidden_size]
        let seq_len_dim = shape[1] as usize;
        let hidden_size = shape[2] as usize;

        let mut embeddings = Vec::with_capacity(batch_size);

        for batch_idx in 0..batch_size {
            let encoding = &encodings[batch_idx];
            let seq_len = encoding.get_ids().len().min(MAX_SEQ_LENGTH).min(seq_len_dim);

            // Mean pooling over non-padding tokens
            let mut embedding = vec![0.0f32; hidden_size];
            for seq_idx in 0..seq_len {
                let offset = batch_idx * seq_len_dim * hidden_size + seq_idx * hidden_size;
                for hidden_idx in 0..hidden_size {
                    embedding[hidden_idx] += output_data[offset + hidden_idx];
                }
            }

            // Divide by sequence length
            for val in &mut embedding {
                *val /= seq_len as f32;
            }

            // L2 normalize
            let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 0.0 {
                for val in &mut embedding {
                    *val /= norm;
                }
            }

            embeddings.push(embedding);
        }

        Ok(embeddings)
    }

    /// Get the embedding dimension
    pub fn dimension(&self) -> usize {
        EMBEDDING_DIM
    }
}

/// Initialize the global embedding model (call once at startup)
pub fn init_embedding_model(models_dir: &Path) -> Result<()> {
    let mut guard = EMBEDDING_MODEL.lock().map_err(|e| Error::Other(e.to_string()))?;
    if guard.is_none() {
        *guard = Some(EmbeddingModel::load(models_dir)?);
    }
    Ok(())
}

/// Get the global embedding model (must call init_embedding_model first)
pub fn get_embedding_model() -> Result<std::sync::MutexGuard<'static, Option<EmbeddingModel>>> {
    EMBEDDING_MODEL
        .lock()
        .map_err(|e| Error::Other(format!("Failed to lock embedding model: {}", e)))
}

/// Generate embedding using the global model
pub fn embed_text(text: &str) -> Result<Vec<f32>> {
    let mut guard = get_embedding_model()?;
    let model = guard
        .as_mut()
        .ok_or_else(|| Error::Other("Embedding model not initialized".to_string()))?;
    model.embed(text)
}

/// Generate embeddings for multiple texts using the global model
pub fn embed_texts(texts: &[&str]) -> Result<Vec<Vec<f32>>> {
    let mut guard = get_embedding_model()?;
    let model = guard
        .as_mut()
        .ok_or_else(|| Error::Other("Embedding model not initialized".to_string()))?;
    model.embed_batch(texts)
}

/// Get the embedding dimension
pub fn embedding_dimension() -> usize {
    EMBEDDING_DIM
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    #[ignore] // Requires model download
    fn test_embedding_generation() {
        let models_dir = PathBuf::from("data/models");
        let mut model = EmbeddingModel::load(&models_dir).unwrap();

        let embedding = model.embed("Hello, world!").unwrap();
        assert_eq!(embedding.len(), EMBEDDING_DIM);

        // Check normalization
        let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.01);
    }

    #[test]
    #[ignore] // Requires model download
    fn test_batch_embedding() {
        let models_dir = PathBuf::from("data/models");
        let mut model = EmbeddingModel::load(&models_dir).unwrap();

        let texts = vec!["Hello", "World", "Rust programming"];
        let embeddings = model.embed_batch(&texts).unwrap();

        assert_eq!(embeddings.len(), 3);
        for emb in &embeddings {
            assert_eq!(emb.len(), EMBEDDING_DIM);
        }
    }
}
