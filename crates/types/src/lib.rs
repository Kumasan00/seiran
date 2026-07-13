//! 共通型定義モジュール
//!
//! このモジュールは、Seiran の核となる **19 フォント種別システム** と、
//! PDF 生成全体で使用される共通型を定義します。
//!
//! ## モジュール構成
//!
//! - `font` - フォント種別 [`FontType`]（19 種別）とスタイル分類 [`FontKind`]（13 種別）
//! - `font_map` - 全フォント種別に値を対応付ける汎用コンテナ [`FontMap`]
//! - `heading_level` - 見出しのレベル [`HeadingLevel`]
//! - [`length`] - 単位付き長さ値 [`Length`]
//! - `math` - 数式環境の種別 [`MathEnvKind`] / 区切り括弧 [`MathDelimiter`] / 記号の数式クラス [`MathClass`]
//! - `table` - 表の列指定 [`TableColumn`] / [`ColumnAlign`] / [`ColumnWidth`]
//! - `link` - ハイパーリンク機構の到達先 [`AnchorMark`]（機構 A）とリンク行き先 [`LinkTarget`]（機構 B）
//! - `color` - 8bit RGB 色 [`Color`]（背景色 / 罫線色 / テキスト色の共通表現）
//! - `align` - 水平方向の揃え [`Align`]（段落・行の左 / 中央 / 右寄せ）
//! - `text_alignment` - 本文段落の行末処理 [`TextAlignment`]（両端揃え / 左揃え）
//! - `theorem` - ビルトイン定理クラス [`TheoremClass`]（固定 10 種）
//!
//! ## 配置基準 — どんな型を置き、どんな型を置かないか
//!
//! **判定テスト**: 「この型の共有範囲は、types 以外に共通の下位クレートを持つか？」
//!
//! - 持たない（`document` に依存できないクレート — `read_config` / `read_style` / `font` / `hlist` —
//!   と、`document` 系 — `parser` / `citation` / `lowering` — の両方が使い、types が唯一の合流点になる）
//!   → **types に置く**
//! - `document` 系だけが使う → **`document` に置く**（例: `QuoteKind`）
//! - 1 クレートだけが使う → **そのクレートに置く**
//! - 補助判定: `LayoutNode` に乗って lowering を生き延びる型は types に置く
//!   （`layout` は意図的に `document` 非依存のため。例: `MathEnvKind`）
//!
//! 置くのは**語彙型**まで — 小さな `Copy` 値型・enum と、その正準変換
//! （`as_str` / `from_name` / serde / `Display`）・純粋演算（[`Length`] の算術、[`Align::offset`]）。
//! IR ノード（`document`）・設定スキーマ（`read_style` / `read_config`）・組版データ構造（`hlist`）・
//! フォントや I/O など挙動を持つ処理は置かない。
//!
//! 例外: [`MathClass`] は現在 `parser` のみが使う先行配置
//! （数式スペーシング実装時に `MathNode` 経由で `layout` まで届く予定のため）。
//!
//! 詳細な導出と各型の判定例は `docs/architecture.md` の types 節を参照。
//!
//! ## `FontMap<T>`
//!
//! [`FontMap`] は 19 フォント種別それぞれに値を対応付ける汎用コンテナです。
//! 内部実装は `HashMap<FontType, T>` で、イテレーションは [`FontType::ALL`] の
//! 順序で返されます。
//!
//! ## 19 フォント種別システム
//!
//! Seiran では、すべてのテキストを以下の 19 フォント種別に分類して処理します：
//!
//! | カテゴリ         | 種別数 | フォント                                              |
//! | -------------- | ------ | -------------------------------------------------- |
//! | Serif (セリフ)  | 4      | 標準、太字、イタリック、太字イタリック                  |
//! | Sans Serif     | 4      | 標準、太字、イタリック、太字イタリック                  |
//! | Monospace      | 4      | 標準、太字、イタリック、太字イタリック                  |
//! | Math (数式)    | 1      | 数式用フォント（OpenType Math テーブル対応）            |
//! | Japanese       | 6      | Serif (標準/太字) + Sans Serif (標準/太字) + Monospace (標準/太字) |
//!
//! 各文字が解析される際、その属性（言語、スタイル、用途）に基づいて
//! 適切なフォント種別が決定されます。
//!
//! - **`FontType`**: すべてのフォント種別（19 個）
//! - **`FontKind`**: 言語判定前のスタイル分類（13 個、日本語フォント除外）
//!
//! 関連クレート：
//! - 設定: `read_config`
//! - フォント処理: `font`
//! - スクリプト判定と `FontKind` → `FontType` の解決: `layout`

mod align;
mod color;
mod font;
mod font_map;
mod heading_level;
// length は garde 用バリデータ（`length::non_negative` / `length::positive`）を
// モジュール名前空間ごと公開するため pub を維持する
pub mod length;
mod link;
mod math;
mod table;
mod text_alignment;
mod theorem;

pub use align::Align;
pub use color::Color;
pub use font::{FontKind, FontType};
pub use font_map::{FontMap, FontMapIter, FontMapIterMut};
pub use heading_level::HeadingLevel;
pub use length::{Length, ParseLengthError};
pub use link::{AnchorMark, LinkTarget};
pub use math::{MathClass, MathDelimiter, MathEnvKind};
pub use table::{ColumnAlign, ColumnWidth, TableColumn};
pub use text_alignment::TextAlignment;
pub use theorem::TheoremClass;
