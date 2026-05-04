mod openai_compatible;

pub use openai_compatible::OpenAiCompatibleModel;

pub fn crate_name() -> &'static str {
    "agent-model-provider"
}
