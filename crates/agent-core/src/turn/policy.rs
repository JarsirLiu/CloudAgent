#[derive(Clone, Debug)]
pub struct ExecutionPolicy {
    pub max_tool_roundtrips: Option<usize>,
}

impl ExecutionPolicy {
    pub fn new(max_tool_roundtrips: Option<usize>) -> Self {
        Self {
            max_tool_roundtrips: max_tool_roundtrips.map(|value| value.max(1)),
        }
    }
}
