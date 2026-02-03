use serde::{Deserialize, Serialize};

/// フィードスケルトンのレスポンス型
#[derive(Debug, Serialize, Deserialize)]
pub struct FeedSkeletonResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub feed: Vec<FeedItem>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FeedItem {
    pub post: String,
}

/// フィードサービス名の列挙型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedService {
    Helloworld,
    Todoapp,
    Oneyearago,
}

impl FeedService {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "helloworld" => Some(Self::Helloworld),
            "todoapp" => Some(Self::Todoapp),
            "oneyearago" => Some(Self::Oneyearago),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Helloworld => "helloworld",
            Self::Todoapp => "todoapp",
            Self::Oneyearago => "oneyearago",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DescribeFeedGeneratorResponse {
    pub did: String,
    pub feeds: Vec<FeedUri>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FeedUri {
    pub uri: String,
}
