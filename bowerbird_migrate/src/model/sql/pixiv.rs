use sqlx::FromRow;

#[derive(FromRow, Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub is_followed: bool,
    pub total_following: Option<i64>,
    pub total_illust_series: Option<i64>,
    pub total_illusts: Option<i64>,
    pub total_manga: Option<i64>,
    pub total_novel_series: Option<i64>,
    pub total_novels: Option<i64>,
    pub total_public_bookmarks: Option<i64>,
}
