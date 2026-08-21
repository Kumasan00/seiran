//! 組版 module — 意味解析の成果物（`semantics::SemanticDocument`）を、描画直前の確定レイアウト
//! （[`LaidOutDocument`]）へ変換する（旧 `typeset` crate、#307 で `seiran` の非公開 module として吸収）
//!
//! 外向きの入口は [`layout`] の 1 操作だけで、段順序（lowering → 計測 → 画像寸法確定 → 行分割・
//! 改ページ → 前付け・後付け → ページラベル → 走り文 → outline）と、その間に成立する不変条件
//! （box 計測は 1 回だけ・`breaking` はフォントに触れない）はすべて実装側に閉じる（#350）。
//!
//! 組版中間型（`Block` / `HItem` / `Line` / `Page` / `TableBox` 系）は本 module 非公開の
//! 子 module `boxes` が所有する（#280、#350 で `layout` から改名）。`compiler::publication` が
//! `Publication` へ写すために読むぶんだけを facade へ出す。
//!
//! フォント処理（OpenType 解析・検証・メトリクス・シェイピング）は子 module `font` が持つ
//! （#352）。入力の 19 種別・設定・バイト列は `project::font` の所有で、この module はそこから
//! フォント資源を組み立てて使う側になる。

use crate::project::ProjectSource;

mod block;
mod boxes;
mod breaking;
mod error;
mod font;
mod geometry;
mod image;
mod lowering;
mod pagination;
mod warning;

// 確定ページ列の決定的テキストダンプ（golden 比較用）。走査対象が `boxes` の中間型なので所有は
// こちら側で、`compiler::golden` は `dump_pages` の 1 関数だけを借りる（#353）。
#[cfg(test)]
mod dump;
// 外側の module のテストが確定レイアウトを組み立てるための fixture builder（#353）。
#[cfg(test)]
pub(crate) mod test_fixtures;

// 確定レイアウトを `Publication` へ写す `compiler::publication` が、ページの中身（配置済みブロック・
// 表の行・箱の内容）を走査するために名指しする型。この `boxes` からの
// 再エクスポートに載るのは **本体コードに消費者がある名前だけ**で、テストが確定レイアウトを
// 組み立てる手段は `#[cfg(test)]` の子 module `test_fixtures` が持つ（#353）。組版の段を呼ぶための型
// （`PageGeometry` / `KnuthPlassBreaker` / 各段の入力）は入口が `layout` 1 操作になったので
// facade から外した（#350）。同様に `Align` / `FootnoteId` も `typeset` の外に消費者がいない（#326）。
pub(crate) use boxes::{AnchorId, AnchorMark, HBoxContent, LinkTarget, Page, PlacedBlock, PlacedTableRow};
// テスト専用の例外 — `compiler::golden` / `compiler::project_source_equivalence` が確定ページ列を
// ダンプ比較するための関数 1 つだけを出す（中間型そのものは出さない）。
#[cfg(test)]
pub(crate) use dump::dump_pages;
pub(crate) use error::TypesetError;
// `Publication` に載って crate 外（描画バックエンド）まで届く leaf 値型 — シェイピング結果
// （`GlyphRun` / `Glyph`）・フォント計測値（`FontMetric`）・krilla フォント構築設定
// （`FontFaceConfig` / `VariationAxisConfig`）。crate root の facade が再エクスポートする（#372）。
pub use font::{FontFaceConfig, FontMetric, Glyph, GlyphRun, VariationAxisConfig};
// フォント資源のハンドル `FontResources`。フォントの解析・検証・シェーパー構築は `font` に
// 閉じており、`FontSystem` / `FontRefs` / `FontMetrics` / 拡張 trait は `typeset` の外から
// 見えない（#352）。
pub(crate) use font::{FontResources, FontWarning};
// 入口は `layout` 1 操作という原則の意図した例外（#351）。用紙・余白 × 段組みの横断制約は
// 組版の不変条件なのでここが所有するが、**呼び出しは入力読込（`compiler::input::load`）の中**で
// 行う — 不正な組み合わせを組版より前に弾き、診断の出るタイミングを変えないため。
pub(crate) use geometry::{LayoutValidationError, validate_layout};
// 画像資源 — 判定済みの形式 `ImageFormat` は `Publication` に載って描画バックエンドまで届く
// leaf 値型（crate root の facade が再エクスポートする）。`ImageAsset` は `compiler` が
// 描画資源へ写すためだけに読む中間表現（#378）。
pub(crate) use image::ImageAsset;
pub use image::ImageFormat;
pub(crate) use pagination::LaidOutDocument;
// 組版が見つけた、ユーザーが直せる非致命的問題（#382）。`compiler` が `Warnings` へ積む。
pub(crate) use warning::TypesetWarning;

use crate::{failures::Failures, project::config::ProjectConfig, semantics::SemanticDocument, style::Style};

/// 意味解析の成果物を、描画直前の確定レイアウトへ組版する。
///
/// 画像は `document` が参照しているぶんだけを `source` 経由で読み込み、自然寸法から表示寸法を
/// 確定して結果へ同梱する（生バイト列は描画の資源束が要求する）。フォント資源は呼び出し元が
/// 構築したものを借り、そこからシェーパーを組むのはこの中（`font` module に閉じる、#352）。
///
/// 組版を止めないがユーザーが直せる問題（脚注のはみ出し）は [`TypesetWarning`] として確定レイアウトと
/// 一緒に返す（#382）。警告は資源でも確定レイアウトの一部でもないので `LaidOutDocument` には持たせず、
/// `FontResources::load` と同じくタプルの第 2 要素にする。
///
/// # Errors
///
/// シェーパーの構築、画像の読込・デコード・寸法確定、または脚注のページ単位採番の収束に
/// 失敗した場合に、その段で見つかった失敗を非空集合で返す（フォント・画像はそれぞれ独立に
/// 検査できるので段の中では全件、段の間は早期 return する）。
pub(crate) fn layout(
  source: &dyn ProjectSource,
  config: &ProjectConfig,
  style: &Style,
  font_resources: &FontResources<'_>,
  document: &SemanticDocument,
) -> Result<(LaidOutDocument, Vec<TypesetWarning>), Failures<TypesetError>> {
  // シェーパー構築は画像読込より前に置く — 両方が失敗する入力で報告されるエラーを、
  // フォント資源を呼び出し元が組んでいた頃と同じ側（フォント）に保つため。
  let font_system = font_resources.system().map_err(|failures| return failures.map(TypesetError::from))?;
  let image_paths = image::collect_image_paths(document.hir());
  let images = image::load_image_resources(source, &image_paths)?;
  let ctx = pagination::TypesetContext::new(config, style, &font_system);
  return pagination::paginate(&ctx, document, images, image_paths).map_err(Failures::single);
}
