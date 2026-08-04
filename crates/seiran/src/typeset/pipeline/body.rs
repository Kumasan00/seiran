//! 本文パス（lowering → 計測 → 画像サイズ確定 → 改行・改ページ）

use std::time::Instant;

use miette::Diagnostic;
use model::Length;
use thiserror::Error;
use tracing::{debug_span, info};

use crate::{
  font::FontSystem,
  typeset::{
    block::build_blocks,
    breaking::{LineBreaker, PageGeometry, break_pages},
    layout::{Block, Page},
    lowering::{HeadingRecord, LoweringContext, lower_sources_with_headings},
  },
};

/// `layout_body` に渡す、脚注ページ単位採番の反復を跨いで変わらない入力。
pub struct BodyLayoutInput<'a> {
  /// 画像既定値（最大 DPI・ダウンサンプル）の出所となる文書設定
  pub config: &'a crate::config::Config,
  /// 本文の書式を決めるスタイル設定
  pub style: &'a crate::config::Style,
  /// シェイプ・メトリクス取得の窓口
  pub resources: &'a FontSystem<'a>,
  /// 本文の版面幅（pt）
  pub text_width: Length,
  /// 本文のページジオメトリ
  pub geometry: &'a PageGeometry,
  /// 使用する行分割アルゴリズム
  pub breaker: &'a dyn LineBreaker,
}

/// 本文パス 1 回ぶんの出力。
#[derive(Debug)]
pub struct BodyLayout {
  /// 確定した本文ページ列
  pub pages: Vec<Page>,
  /// 目次・しおり用の見出し情報（文書順）
  pub headings: Vec<HeadingRecord>,
}

/// `layout_body` の失敗理由。
///
/// 画像解決の失敗型はジェネリクス `E` で受け取り、`typeset` が呼び出し元（`seiran`）の
/// エラー型を名指しで知らない状態を保つ。lowering は解決済みツリーを描くだけになり
/// （ラベル・カウンタ解決は `resolve` クレートが上流で済ませる）失敗しなくなったため、
/// この enum に残る失敗理由は画像解決だけになった。
#[derive(Debug, Error, Diagnostic)]
pub enum BodyLayoutError<E: std::error::Error + Diagnostic + 'static> {
  /// 画像サイズの解決（`resolve_images` コールバック）に失敗した場合に返される。
  #[error("画像サイズの解決に失敗しました")]
  #[diagnostic(code(pipeline::body::image_resolution))]
  Images {
    /// 元の画像解決エラー（呼び出し元の型）
    #[source]
    #[diagnostic_source]
    source: E,
  },
}

/// 経過時間をミリ秒で返す。
///
/// 組版処理時間が `u64::MAX` ms（約 5 億年）を超えることはない前提。
#[allow(clippy::cast_possible_truncation)]
fn elapsed_ms(start: Instant) -> u64 { return start.elapsed().as_millis() as u64; }

/// 本文を組版し、確定ページ列と見出し記録を返す。
///
/// lowering → `build_blocks` → 画像サイズ確定（`resolve_images` コールバック経由）→
/// `break_pages` を 1 呼び出しに畳む。`footnote_numbers` は出現順で引く脚注番号の上書き列
/// （ページ単位採番の不動点反復で複数回呼ばれる）。
///
/// # Errors
///
/// 画像解決に失敗した場合にエラーを返す（lowering は解決済みツリーを描くだけなので失敗しない）。
pub fn layout_body<E: std::error::Error + Diagnostic + 'static>(
  input: &BodyLayoutInput<'_>,
  document: &crate::resolve::ResolvedDocument,
  footnote_numbers: Option<&[u32]>,
  resolve_images: impl FnOnce(Vec<Block>) -> Result<Vec<Block>, E>,
) -> Result<BodyLayout, BodyLayoutError<E>> {
  let stage_start = Instant::now();
  let mut lowering_ctx =
    LoweringContext::new(input.style).with_image_defaults(input.config.image.max_dpi, input.config.image.downsample);
  if let Some(numbers) = footnote_numbers {
    lowering_ctx = lowering_ctx.with_footnote_numbers(numbers);
  }
  let (body_layout_nodes, headings) = lower_sources_with_headings(&lowering_ctx, document);
  info!(elapsed_ms = elapsed_ms(stage_start), "解決済みドキュメント → LayoutNode への変換が完了しました");

  let stage_start = Instant::now();
  let body_blocks = {
    let _span = debug_span!("build_blocks", region = "body").entered();
    build_blocks(
      body_layout_nodes,
      input.resources,
      input.style.text.font_size,
      input.style.text.line_height_factor,
      input.config.document.language.as_deref(),
      input.style.text.punctuation_spacing,
    )
  };
  info!(
    block_count = body_blocks.len(),
    elapsed_ms = elapsed_ms(stage_start),
    "本文ブロックの構築が完了しました"
  );

  let stage_start = Instant::now();
  let body_blocks = resolve_images(body_blocks).map_err(|source| return BodyLayoutError::Images { source })?;
  info!(elapsed_ms = elapsed_ms(stage_start), "画像サイズの確定が完了しました");

  let stage_start = Instant::now();
  let pages = {
    let _span = debug_span!("break_pages", region = "body").entered();
    break_pages(body_blocks, input.text_width, input.geometry, input.breaker, input.style.text.alignment)
  };
  info!(
    body_page_count = pages.len(),
    elapsed_ms = elapsed_ms(stage_start),
    "本文のページ分割が完了しました"
  );
  return Ok(BodyLayout { pages, headings });
}
