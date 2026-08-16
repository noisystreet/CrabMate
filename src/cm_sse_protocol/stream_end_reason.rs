#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamEndReason {
    Completed,
    Cancelled,
    Conflict,
    Fallback,
    NoOutput,
    Gone,
}

impl StreamEndReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Conflict => "conflict",
            Self::Fallback => "fallback",
            Self::NoOutput => "no_output",
            Self::Gone => "gone",
        }
    }

    fn parse_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "completed" => Some(Self::Completed),
            "cancelled" => Some(Self::Cancelled),
            "conflict" => Some(Self::Conflict),
            "fallback" => Some(Self::Fallback),
            "no_output" => Some(Self::NoOutput),
            "gone" => Some(Self::Gone),
            _ => None,
        }
    }

    pub fn label_zh_hans(self) -> &'static str {
        match self {
            Self::Completed => "完成",
            Self::Cancelled => "已取消",
            Self::Conflict => "会话冲突",
            Self::Fallback => "补偿收尾",
            Self::NoOutput => "无输出结束",
            Self::Gone => "流已回收",
        }
    }

    pub fn label_en(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Conflict => "conflict",
            Self::Fallback => "fallback",
            Self::NoOutput => "no output",
            Self::Gone => "gone",
        }
    }
}

impl core::fmt::Display for StreamEndReason {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl core::str::FromStr for StreamEndReason {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse_str(s).ok_or(())
    }
}

#[cfg(test)]
mod tests {
    use super::StreamEndReason;

    #[test]
    fn parse_and_labels_roundtrip() {
        let all = [
            StreamEndReason::Completed,
            StreamEndReason::Cancelled,
            StreamEndReason::Conflict,
            StreamEndReason::Fallback,
            StreamEndReason::NoOutput,
            StreamEndReason::Gone,
        ];
        for reason in all {
            let s = reason.as_str();
            assert_eq!(s.parse::<StreamEndReason>().ok(), Some(reason));
            assert!(!reason.label_zh_hans().is_empty());
            assert!(!reason.label_en().is_empty());
        }
    }
}
