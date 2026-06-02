//! レイアウトエンジンモジュール
//!
//! パーサーが生成した `LayoutNode` をフォントシェーピング・メトリクス情報と組み合わせて、
//! PDF 生成に使用される `Item`（Box/Glue/Penalty）リストに変換します。
//!
//! ## アーキテクチャ上の位置づけ
//!
//! このクレートは `parser`（フォント非依存）と `font`（フォント処理）の間の橋渡し役です。
//!
//! ```text
//! parser (DocNode) ──→ layout (lowering → LayoutNode → Item) ──→ pdf_gen (PDF bytes)
//!                              ↑
//!                         font (shaping, metrics)
//! ```
//!
//! ## データフロー
//!
//! 1. `parser::parse_source` が生成した `Vec<DocNode>` を受け取る
//! 2. `lowering` モジュールで `DocNode → LayoutNode` に変換
//! 3. テキストノードを Unicode スクリプトに基づいて分割し、適切なフォント種別を割り当てる
//! 4. 各セグメントをフォントシェーパーでシェーピングし、グリフ情報を取得
//! 5. Box/Glue/Penalty モデルの `Item` リストに変換して返す

mod layout_engine;
pub mod layout_node;
pub mod lowering;

pub use layout_engine::{BoxItem, Glyph, GlyphRun, Item, layout_engine};
pub use layout_node::{LayoutNode, Style};
pub use lowering::{LoweringContext, LoweringError, lower_document, lower_nodes};
