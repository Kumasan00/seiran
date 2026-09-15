//! CST（具象構文木）の表現

// 3 子は親 `syntax` が `pub(super) use` で `frontend` 幅に再輸出する。ここで `pub(super)` と書くと
// `syntax` までしか届かず再輸出できない（E0365）ので、再輸出と同じ幅を `pub(in ...)` で書く。
pub(in crate::frontend) mod green;
pub(in crate::frontend) mod kind;
pub(in crate::frontend) mod view;
