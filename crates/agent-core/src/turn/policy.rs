#[derive(Clone, Debug)]
pub struct ExecutionPolicy {
    pub max_tool_roundtrips: usize,
}

impl ExecutionPolicy {
    pub fn new(max_tool_roundtrips: usize) -> Self {
        Self {
            max_tool_roundtrips: max_tool_roundtrips.max(1),
        }
    }
}
