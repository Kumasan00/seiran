use font::{
  font_info::FontInfos,
  glyph_mapping::{GlyphMapping, GlyphMappings},
};
use layout::{BoxItem, Glyph, Item};
use pdf_writer::{Content, Name, Str};
use read_config::Config;
use read_style::Style;

pub(crate) struct PDFContent {
  pub content: Content,
  pub annotations: Vec<Annotation>,
}

pub(crate) struct Annotation;

/// PDFコンテンツストリームを生成する
///
/// レイアウトエンジンが生成したアイテムを基に、
/// 各ページのPDFコンテンツストリームを構築します。
///
/// # Arguments
///
/// * `config` - PDF生成設定
/// * `items` - レイアウトエンジンから生成されたアイテム
/// * `glyph_mappings` - グリフマッピング情報
/// * `font_infos` - フォントメタデータ情報
///
/// # Returns
///
/// 各ページのPDFコンテンツ
pub(crate) fn create_pdf_contents(
  config: &Config,
  style: &Style,
  items: &[Item],
  glyph_mappings: &GlyphMappings,
  font_infos: &FontInfos,
) -> Vec<PDFContent> {
  let font_configs = &config.font_configs;
  let page_height = config.pdf.height;
  let margin = &config.pdf.margin;
  let line_height_factor = config.pdf.line_height_factor;

  let mut pdf_contents: Vec<PDFContent> = Vec::new();
  let mut content = setup_content(config);

  // 現在位置（PDF座標系：左下原点、Y軸上向き）
  let start_x = margin.left;
  let start_y = page_height - margin.top;
  let mut x = start_x;
  let mut y = start_y;

  // 直前のフォントサイズ（Glueスケーリング・改行幅計算用）
  let mut current_font_size = style.font_size;

  for item in items {
    match item {
      Item::Box(BoxItem::Text(run)) => {
        let font_config = font_configs.get(run.font_type);
        let font_name = &font_config.font_name;
        let glyph_mapping = glyph_mappings.get(run.font_type);
        let font_info = font_infos.get(run.font_type);
        let upem = f32::from(font_info.upem);
        let new_font_size = run.font_size;

        // テキストブロックを開始し、フォントと位置を設定
        content.begin_text();
        content.set_font(Name(font_name.as_bytes()), run.font_size);
        content.next_line(x, y);

        // グリフに位置調整（diff）があるかチェック
        let has_diff = run.glyphs.iter().any(|g| g.diff.is_some());

        if has_diff {
          // TJオペレータで位置調整付き表示
          show_glyphs_positioned(&mut content, &run.glyphs, glyph_mapping);
        } else {
          // Tjオペレータで一括表示
          let text_buffer = build_cid_bytes(&run.glyphs, glyph_mapping);
          content.show(Str(&text_buffer));
        }

        content.end_text();

        // x位置を更新
        // CIDFont のグリフ幅スケーリング: glyph_units * font_size / upem
        #[allow(clippy::cast_precision_loss)]
        {
          x += run.width as f32 * run.font_size / upem;
        }

        current_font_size = new_font_size;
      },

      Item::Box(BoxItem::Rule { width, height }) => {
        // 罫線を描画
        content.save_state();
        content.set_fill_rgb(0.0, 0.0, 0.0);
        content.rect(x, y - height, *width, *height);
        content.fill_nonzero();
        content.restore_state();
        x += width;
      },

      Item::Glue { natural, .. } => {
        // 単語間スペース（font design units → PDF points）
        x += natural * current_font_size / 1000.0;
      },

      Item::Kern(kern_value) => {
        // 水平カーニング（ポイント単位）
        x += kern_value;
      },

      Item::Vkern(vkern_value) => {
        // 垂直スペース
        y -= vkern_value + current_font_size;
        x = start_x;
      },

      Item::Penalty(penalty) => {
        if *penalty == i32::MIN {
          // ページブレイク：現在のページを保存し新しいページを開始
          pdf_contents.push(PDFContent {
            content,
            annotations: Vec::new(),
          });
          content = setup_content(config);
          x = start_x;
          y = start_y;
        } else if *penalty <= -1000 {
          // 強制改行
          let line_height = current_font_size * line_height_factor;
          y -= line_height;
          x = start_x;

          // ページ下端超過チェック：自動改ページ
          if y < margin.bottom {
            pdf_contents.push(PDFContent {
              content,
              annotations: Vec::new(),
            });
            content = setup_content(config);
            x = start_x;
            y = start_y;
          }
        }
      },
    }
  }

  // 最後のページを追加
  pdf_contents.push(PDFContent {
    content,
    annotations: Vec::new(),
  });

  return pdf_contents;
}

/// グリフ配列からCIDバイト列を構築する
///
/// 各グリフのGIDをCIDに変換し、ビッグエンディアン16ビットのバイト列として返します。
///
/// # Arguments
///
/// * `glyphs` - グリフ配列
/// * `glyph_mapping` - GID→CIDマッピング
///
/// # Returns
///
/// CIDバイト列
fn build_cid_bytes(glyphs: &[Glyph], glyph_mapping: &GlyphMapping) -> Vec<u8> {
  let mut buffer = Vec::with_capacity(glyphs.len() * 2);
  for glyph in glyphs {
    let cid = glyph_mapping.old_to_cid[glyph.gid as usize].unwrap_or(0);
    buffer.extend_from_slice(&cid.to_be_bytes());
  }
  return buffer;
}

/// グリフを位置調整付きで表示する（TJオペレータ）
///
/// `HarfBuzz` のシェーピング結果とフォントのhmtxアドバンス幅に差分がある場合、
/// TJオペレータの数値調整を使って正確なグリフ配置を行います。
///
/// # Arguments
///
/// * `content` - PDFコンテンツストリーム
/// * `glyphs` - グリフ配列
/// * `glyph_mapping` - GID→CIDマッピング
fn show_glyphs_positioned(content: &mut Content, glyphs: &[Glyph], glyph_mapping: &GlyphMapping) {
  let mut positioned = content.show_positioned();
  let mut items = positioned.items();
  let mut text_buffer: Vec<u8> = Vec::new();

  for glyph in glyphs {
    let cid = glyph_mapping.old_to_cid[glyph.gid as usize].unwrap_or(0);
    text_buffer.extend_from_slice(&cid.to_be_bytes());

    if let Some(diff) = glyph.diff {
      // 蓄積されたテキストを表示
      items.show(Str(&text_buffer));
      text_buffer.clear();
      // 位置調整：TJの正値は左移動（減算）、負値は右移動
      // diff = shaped_advance - hmtx_advance なので、-diff で補正
      #[allow(clippy::cast_precision_loss)]
      {
        items.adjust(-diff as f32);
      }
    }
  }

  if !text_buffer.is_empty() {
    items.show(Str(&text_buffer));
  }
}

/// ページの初期コンテンツストリームを作成する
///
/// 背景色が設定されている場合、ページ全体を塗りつぶします。
///
/// # Arguments
///
/// * `config` - PDF生成設定
///
/// # Returns
///
/// 初期化済みのコンテンツストリーム
fn setup_content(config: &Config) -> Content {
  let mut content = Content::new();
  if let Some((r, g, b)) = config.pdf.background_color {
    content.save_state();
    content.set_fill_rgb(r, g, b);
    content.rect(0.0, 0.0, config.pdf.width, config.pdf.height);
    content.fill_nonzero();
    content.restore_state();
  }

  return content;
}
