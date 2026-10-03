pub struct Question {
    pub text: String,
    pub choices: Vec<String>,
}

#[derive(Debug)]
pub struct Answer {
    pub choice: String,
    pub confidence: f32,
}

pub fn ask(situation: &str, questions: &[Question]) -> Result<Vec<Answer>, String> {
    if situation.is_empty() {
        return Err("situation is empty".to_string());
    }

    if questions.is_empty() {
        return Err("questions are empty".to_string());
    }

    for question in questions {
        if question.text.is_empty() {
            return Err("question is empty".to_string());
        }

        if question.choices.is_empty() {
            return Err("choices are empty".to_string());
        }

        for choice in &question.choices {
            if choice.is_empty() {
                return Err("choice is empty".to_string());
            }
        }
    }

    #[cfg(not(feature = "ai"))]
    {
        return Err("ai is not built".to_string());
    }

    #[cfg(feature = "ai")]
    {
        score(situation, questions)
    }
}

fn softmax(scores: &[f32]) -> Vec<f32> {
    if scores.is_empty() {
        return Vec::new();
    }

    let max = scores.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut weights = Vec::with_capacity(scores.len());
    let mut sum = 0.0f32;

    for score in scores {
        let weight = (*score - max).exp();
        weights.push(weight);
        sum += weight;
    }

    if sum == 0.0 {
        let even = 1.0 / scores.len() as f32;

        return vec![even; scores.len()];
    }

    for weight in &mut weights {
        *weight /= sum;
    }

    weights
}

fn log_softmax_at(logits: &[f32], token: i32) -> Result<f32, String> {
    let idx = usize::try_from(token).map_err(|_| "token id is negative".to_string())?;
    let logit = *logits
        .get(idx)
        .ok_or_else(|| "token is outside the vocab".to_string())?;
    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut sum = 0.0f32;

    for value in logits {
        sum += (*value - max).exp();
    }

    if sum == 0.0 {
        return Err("logits are empty".to_string());
    }

    Ok(logit - max - sum.ln())
}

fn winning_index(probs: &[f32]) -> usize {
    let mut best = 0usize;

    for idx in 1..probs.len() {
        if probs[idx] > probs[best] {
            best = idx;
        }
    }

    best
}

#[cfg(feature = "ai")]
fn answer_for(choices: &[String], scores: &[f32]) -> Answer {
    let probs = softmax(scores);
    let best = winning_index(&probs);

    Answer {
        choice: choices[best].clone(),
        confidence: probs[best],
    }
}

#[cfg(feature = "ai")]
mod infer {
    use super::{answer_for, log_softmax_at, Answer, Question};
    use llama_cpp_2::context::params::LlamaContextParams;
    use llama_cpp_2::context::LlamaContext;
    use llama_cpp_2::llama_backend::LlamaBackend;
    use llama_cpp_2::llama_batch::LlamaBatch;
    use llama_cpp_2::model::params::LlamaModelParams;
    use llama_cpp_2::model::LlamaModel;
    use llama_cpp_2::token::LlamaToken;
    use llama_cpp_2::vocab::LlamaVocab;
    use std::cell::RefCell;
    use std::num::NonZeroU32;
    use std::path::PathBuf;

    const N_CTX: u32 = 512;

    struct Engine {
        ctx: LlamaContext<'static>,
    }

    pub fn score(situation: &str, questions: &[Question]) -> Result<Vec<Answer>, String> {
        with_engine(|engine| decode_questions(engine, situation, questions))
    }

    fn with_engine<T>(body: impl FnOnce(&mut Engine) -> Result<T, String>) -> Result<T, String> {
        thread_local! {
            static SLOT: RefCell<Option<Engine>> = RefCell::new(None);
        }

        SLOT.with(|slot| {
            let mut slot = slot.borrow_mut();

            if slot.is_none() {
                *slot = Some(load()?);
            }

            body(slot.as_mut().expect("engine"))
        })
    }

    fn load() -> Result<Engine, String> {
        let path = model_path()?;
        let mut backend = LlamaBackend::init().map_err(|err| err.to_string())?;
        backend.void_logs();
        let model = LlamaModel::load_from_file(&backend, &path, &LlamaModelParams::default())
            .map_err(|err| format!("{}: {err}", path.display()))?;
        let model: &'static LlamaModel = Box::leak(Box::new(model));
        let ctx = match model.new_context(&backend, context_params()) {
            Ok(ctx) => ctx,
            Err(err) => {
                std::mem::forget(backend);

                return Err(err.to_string());
            }
        };
        std::mem::forget(backend);
        log::info!("[ai] loaded {}", path.display());

        Ok(Engine { ctx })
    }

    fn context_params() -> LlamaContextParams {
        LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(N_CTX))
            .with_n_batch(N_CTX)
            .with_n_ubatch(N_CTX)
    }

    fn model_path() -> Result<PathBuf, String> {
        if let Ok(path) = std::env::var("AI_MODEL") {
            if !path.is_empty() {
                let path = PathBuf::from(path);

                if path.is_file() {
                    return Ok(path);
                }

                return Err(format!("AI_MODEL {} is missing", path.display()));
            }
        }

        for root in crate::fs::search_roots() {
            let path = root.join("models").join("qwen.gguf");

            if path.is_file() {
                return Ok(path);
            }
        }

        if let Ok(path) = packed_model() {
            return Ok(path);
        }

        Err("missing models/qwen.gguf".to_string())
    }

    fn packed_model() -> Result<PathBuf, String> {
        let bytes = crate::fs::read("models/qwen.gguf")?;
        let path = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join("qwen.gguf")))
            .unwrap_or_else(|| std::env::temp_dir().join("qwen.gguf"));

        if let Ok(meta) = std::fs::metadata(&path) {
            if meta.len() == bytes.len() as u64 {
                return Ok(path);
            }
        }

        let tmp = path.with_extension("gguf.tmp");
        std::fs::write(&tmp, &bytes).map_err(|err| format!("write {}: {err}", tmp.display()))?;
        std::fs::rename(&tmp, &path).map_err(|err| format!("write {}: {err}", path.display()))?;

        Ok(path)
    }

    fn tokenize(vocab: &LlamaVocab, text: &str, add_special: bool) -> Vec<LlamaToken> {
        vocab.tokenize(text.as_bytes(), add_special, false)
    }

    fn rewind(ctx: &mut LlamaContext, pos: i32) -> Result<(), String> {
        let pos = u32::try_from(pos).map_err(|_| "bad position".to_string())?;
        ctx.kv_cache_seq_rm(0, Some(pos), None)
            .map_err(|err| err.to_string())?;

        Ok(())
    }

    fn decode_tokens(
        ctx: &mut LlamaContext,
        batch: &mut LlamaBatch,
        tokens: &[LlamaToken],
        pos: i32,
        want_logits: bool,
    ) -> Result<(), String> {
        if tokens.is_empty() {
            return Err("nothing to decode".to_string());
        }

        let len = i32::try_from(tokens.len()).map_err(|_| "too many tokens".to_string())?;
        let end = pos.checked_add(len).ok_or_else(|| "context exceeded".to_string())?;

        if end > i32::try_from(ctx.n_ctx()).unwrap_or(i32::MAX) {
            return Err("context exceeded".to_string());
        }

        let width = usize::try_from(ctx.n_batch()).unwrap_or(1).max(1);
        let mut offset = 0usize;

        while offset < tokens.len() {
            let count = (tokens.len() - offset).min(width);
            let last = offset + count == tokens.len();
            batch.clear();

            for idx in 0..count {
                let at = pos + i32::try_from(offset + idx).unwrap_or(i32::MAX);
                let logits = want_logits && last && idx + 1 == count;
                batch
                    .add(tokens[offset + idx], at, &[0], logits)
                    .map_err(|err| err.to_string())?;
            }

            ctx.decode(batch).map_err(|err| err.to_string())?;
            offset += count;
        }

        Ok(())
    }

    fn logits_row(ctx: &LlamaContext, index: i32) -> Vec<f32> {
        ctx.get_logits_ith(index).to_vec()
    }

    fn mean_logprob(
        ctx: &mut LlamaContext,
        batch: &mut LlamaBatch,
        prefix: &[f32],
        choice: &[LlamaToken],
        pos: i32,
    ) -> Result<f32, String> {
        if choice.is_empty() {
            return Err("choice produced no tokens".to_string());
        }

        let mut total = log_softmax_at(prefix, choice[0].0)?;

        if choice.len() == 1 {
            return Ok(total);
        }

        let mut cursor = pos;

        for idx in 0..choice.len() - 1 {
            decode_tokens(ctx, batch, &[choice[idx]], cursor, true)?;
            let row = logits_row(ctx, batch.n_tokens() - 1);
            total += log_softmax_at(&row, choice[idx + 1].0)?;
            cursor += 1;
        }

        Ok(total / choice.len() as f32)
    }

    fn decode_questions(
        engine: &mut Engine,
        situation: &str,
        questions: &[Question],
    ) -> Result<Vec<Answer>, String> {
        let vocab = engine.ctx.model.vocab();
        engine.ctx.clear_kv_cache();
        let situation_tokens = tokenize(&vocab, situation, true);

        if situation_tokens.is_empty() {
            return Err("situation produced no tokens".to_string());
        }

        let mut batch = LlamaBatch::new(N_CTX as usize, 1);
        decode_tokens(
            &mut engine.ctx,
            &mut batch,
            &situation_tokens,
            0,
            false,
        )?;
        let sit_end = i32::try_from(situation_tokens.len()).unwrap_or(i32::MAX);
        let mut answers = Vec::with_capacity(questions.len());

        for (question_idx, question) in questions.iter().enumerate() {
            if question_idx > 0 {
                rewind(&mut engine.ctx, sit_end)?;
            }

            let suffix = format!("\n{}\nThe answer is: ", question.text);
            let suffix_tokens = tokenize(&vocab, &suffix, false);

            if suffix_tokens.is_empty() {
                return Err("question produced no tokens".to_string());
            }

            decode_tokens(
                &mut engine.ctx,
                &mut batch,
                &suffix_tokens,
                sit_end,
                true,
            )?;
            let q_end = sit_end
                + i32::try_from(suffix_tokens.len()).unwrap_or(i32::MAX);
            let prefix_logits = logits_row(&engine.ctx, batch.n_tokens() - 1);
            let mut scores = Vec::with_capacity(question.choices.len());
            let mut extended = false;

            for choice in &question.choices {
                if extended {
                    rewind(&mut engine.ctx, q_end)?;
                    extended = false;
                }

                let choice_tokens = tokenize(&vocab, choice, false);

                if choice_tokens.is_empty() {
                    return Err(format!("choice {choice} produced no tokens"));
                }

                let score = mean_logprob(
                    &mut engine.ctx,
                    &mut batch,
                    &prefix_logits,
                    &choice_tokens,
                    q_end,
                )?;

                if choice_tokens.len() > 1 {
                    extended = true;
                }

                scores.push(score);
            }

            answers.push(answer_for(&question.choices, &scores));
        }

        Ok(answers)
    }
}

#[cfg(feature = "ai")]
fn score(situation: &str, questions: &[Question]) -> Result<Vec<Answer>, String> {
    infer::score(situation, questions)
}

#[cfg(test)]
mod tests {
    use super::{ask, log_softmax_at, softmax, winning_index, Question};

    #[test]
    fn softmax_sums_to_one() {
        let probs = softmax(&[1.0, 2.0, 3.0]);
        let sum: f32 = probs.iter().sum();
        assert!((sum - 1.0).abs() < 1.0e-5);
        assert!(probs[2] > probs[1]);
        assert!(probs[1] > probs[0]);
    }

    #[test]
    fn softmax_empty() {
        assert!(softmax(&[]).is_empty());
    }

    #[test]
    fn softmax_equal_scores_are_uniform() {
        let probs = softmax(&[2.0, 2.0]);
        assert!((probs[0] - 0.5).abs() < 1.0e-5);
        assert!((probs[1] - 0.5).abs() < 1.0e-5);
    }

    #[test]
    fn log_softmax_is_a_distribution() {
        let logits = [1.0, 3.0];
        let low = log_softmax_at(&logits, 0).unwrap();
        let high = log_softmax_at(&logits, 1).unwrap();
        assert!(high > low);
        let sum = low.exp() + high.exp();
        assert!((sum - 1.0).abs() < 1.0e-5);
    }

    #[test]
    fn tie_keeps_the_first_choice() {
        assert_eq!(winning_index(&[0.5, 0.5]), 0);
        assert_eq!(winning_index(&[0.2, 0.8]), 1);
    }

    #[test]
    fn ask_rejects_empty_situation() {
        let err = ask("", &[]).unwrap_err();
        assert_eq!(err, "situation is empty");
    }

    #[test]
    fn ask_rejects_empty_questions() {
        let err = ask("The player is close.", &[]).unwrap_err();
        assert_eq!(err, "questions are empty");
    }

    #[test]
    fn ask_rejects_empty_choice() {
        let questions = vec![Question {
            text: "Should I move?".to_string(),
            choices: vec!["yes".to_string(), String::new()],
        }];
        let err = ask("The player is close.", &questions).unwrap_err();
        assert_eq!(err, "choice is empty");
    }

    #[cfg(not(feature = "ai"))]
    #[test]
    fn ask_reports_missing_build() {
        let questions = vec![Question {
            text: "Should I move?".to_string(),
            choices: vec!["yes".to_string(), "no".to_string()],
        }];
        let err = ask("The player is close.", &questions).unwrap_err();
        assert_eq!(err, "ai is not built");
    }
}
