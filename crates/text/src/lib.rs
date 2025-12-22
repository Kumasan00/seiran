//! テキストシェーピングモジュール
//!
//! このモジュールは、HarfBuzzを使用してテキストをグリフシーケンスに変換し、
//! PDF用のコンテンツストリームを生成する機能を提供します。
//! テキストのシェーピング、グリフマッピングの管理、位置調整を行います。

use std::{collections::HashMap, fs, io};

use font::shaper::{HarfRustShaper, HarfRustShapers, ShaperResult};
use indexmap::IndexSet;
use pdf_writer::{Content, Finish, Name, Str};
use read_config_file::Config;
use types::{GlyphMapping, GlyphMappings};

// 定数

/// .notdef グリフのグリフID
const NOTDEF_GID: u16 = 0;

/// シェーピング結果の情報
///
/// HarfBuzzによるテキストシェーピングの結果として得られる、
/// 各グリフのID、位置情報、クラスタ情報を保持します。
/// クラスタは元のテキスト内での文字位置を示します。
#[derive(Debug)]
pub struct ShapingResult {
  /// グリフID
  pub gid: u16,
  /// クラスタ番号（元のテキスト内の位置）
  pub cluster: u32,
  /// 横方向の進み幅
  pub x_advance: i32,
  /// 縦方向の進み幅
  #[allow(dead_code)]
  y_advance: i32,
  /// 横方向のオフセット
  #[allow(dead_code)]
  x_offset: i32,
  /// 縦方向のオフセット
  #[allow(dead_code)]
  y_offset: i32,
}

/// テキストの各行を処理してPDFコンテンツストリームを生成
///
/// 入力テキストの各行に対してシェーピングを実行し、グリフマッピングを更新しながら
/// PDFコンテンツストリームを構築します。背景色の設定、フォントの指定、
/// 行の配置と改行処理も行います。
///
/// # 引数
///
/// * `text_lines` - 処理するテキスト行のイテレータ
/// * `font_contexts` - 全フォントのコンテキスト
/// * `glyph_mappings` - 全フォントのグリフマッピング情報
/// * `config` - アプリケーション設定
///
/// # 戻り値
///
/// PDFコンテンツストリームのベクタを返します（現在は1ページ分）。
///
/// # エラー
///
/// 行の読み込みまたは処理中にエラーが発生した場合にエラーを返します。
pub fn process_text_lines(
  text_lines: io::Lines<io::BufReader<fs::File>>,
  font_contexts: &mut HarfRustShapers,
  glyph_mappings: &mut GlyphMappings,
  config: &Config,
) -> Result<Vec<Content>, Box<dyn std::error::Error>> {
  let mut contents = Vec::new();
  let units_per_em = 1000.0; // TODO: Get from font
  let main_font_context = &mut font_contexts.serif_font;
  let glyph_mapping = &mut glyph_mappings.serif_font;
  let mut pdf_content = Content::new();

  if let Some((r, g, b)) = config.pdf.background_color {
    pdf_content.save_state();
    pdf_content.set_fill_rgb(r, g, b);
    pdf_content.rect(0.0, 0.0, config.pdf.width, config.pdf.height);
    pdf_content.fill_nonzero();
    pdf_content.restore_state();
  }

  pdf_content.begin_text();
  pdf_content.set_font(Name(config.font_configs.serif.font_name.as_bytes()), config.pdf.font_size);
  pdf_content.next_line(config.pdf.margin.left, config.pdf.height - config.pdf.margin.top);

  for (line_num, line) in text_lines.enumerate() {
    let text_line = line?;
    println!("Line {}: {}", line_num + 1, text_line);

    process_single_line(&text_line, main_font_context, units_per_em, glyph_mapping, &mut pdf_content)?;

    let line_height_factor = config.pdf.line_height_factor;
    pdf_content.next_line(0.0, -config.pdf.font_size * line_height_factor);
  }

  pdf_content.end_text();
  contents.push(pdf_content);

  Ok(contents)
}

/// 単一行のテキストを処理してコンテンツストリームに追加
///
/// HarfBuzzを使用してテキストシェーピングを実行し、グリフIDとCIDのマッピングを更新します。
/// 各グリフのアドバンス幅を記録し、フォントの実際の幅とシェーピング結果の差分を調整します。
/// また、.notdefグリフ（フォントに存在しない文字）を検出して警告を出力します。
///
/// # 引数
///
/// * `text_line` - 処理するテキスト行
/// * `main_font_context` - メインフォントのコンテキスト
/// * `units_per_em` - フォントのユニット/em値
/// * `glyph_mapping` - グリフマッピング情報
/// * `pdf_content` - PDFコンテンツストリーム
///
/// # エラー
///
/// シェーピングまたはコンテンツ書き込み中にエラーが発生した場合にエラーを返します。
fn process_single_line(
  text_line: &str,
  main_font_context: &mut HarfRustShaper,
  units_per_em: f32,
  glyph_mapping: &mut GlyphMapping,
  pdf_content: &mut Content,
) -> Result<(), Box<dyn std::error::Error>> {
  let mut position_text = pdf_content.show_positioned();
  let mut items = position_text.items();

  let shape_results = shaping(
    text_line,
    main_font_context,
    &mut glyph_mapping.gid_to_cid,
    &mut glyph_mapping.used_gids,
  )?;

  let mut text_buffer = Vec::new();
  let mut notdef_glyph = IndexSet::new();

  for (glyph_index, shape_result) in shape_results.iter().enumerate() {
    let gid = shape_result.gid;
    let Some(&cid) = glyph_mapping.gid_to_cid.get(&gid) else {
      let fallback_cid = NOTDEF_GID;
      text_buffer.push((fallback_cid >> 8) as u8);
      text_buffer.push((fallback_cid & 0xff) as u8);
      continue;
    };

    let advance_width = 1000.0; // TODO: Get actual advance width from font
    glyph_mapping.advance_widths.entry(cid).or_insert(advance_width);
    let shape_advance = shape_result.x_advance as f32 * 1000.0 / units_per_em;

    text_buffer.push((cid >> 8) as u8);
    text_buffer.push((cid & 0xff) as u8);

    let advance_diff = advance_width - shape_advance;
    if advance_diff.abs() > 0.0001 {
      if !text_buffer.is_empty() {
        items.show(Str(&text_buffer));
        text_buffer.clear();
      }
      items.adjust(advance_diff);
    }

    let char_range = get_char_range(text_line, &shape_results, glyph_index);
    let mut chars: Vec<char> = text_line[char_range].chars().collect();

    if gid == 0 {
      let not_def_chars = chars.clone();
      for c in &not_def_chars {
        notdef_glyph.insert(*c);
      }
    }

    if let Some(existing) = glyph_mapping.cid_to_chars.get_mut(&cid) {
      existing.append(&mut chars);
    } else {
      glyph_mapping.cid_to_chars.insert(cid, chars);
    }
  }

  if !text_buffer.is_empty() {
    items.show(Str(&text_buffer));
  }
  items.finish();
  position_text.finish();

  if !notdef_glyph.is_empty() {
    eprintln!(
      "Warning: The following characters are missing in the font, resulting in .notdef glyphs: {:?}",
      notdef_glyph
    );
  }

  Ok(())
}

/// テキストをシェーピングしてグリフIDとその位置情報を得る
///
/// HarfBuzzを使用してテキストを解析し、各文字に対応するグリフID、
/// 位置情報（アドバンス幅、オフセット）、クラスタ情報を取得します。
/// 新しいグリフが見つかった場合は、GIDからCIDへのマッピングに追加し、
/// 使用グリフの集合を更新します。
///
/// # 引数
///
/// * `text` - シェーピングするテキスト
/// * `hb_font` - HarfBuzzフォントオブジェクト
/// * `gid_to_cid` - GIDからCIDへのマッピング（更新される）
/// * `used_gids` - 使用されたGIDの集合（更新される）
///
/// # 戻り値
///
/// シェーピング結果のベクタを返します。各要素は1つのグリフに対応します。
fn shaping(
  text: &str,
  shaper: &mut HarfRustShaper,
  gid_to_cid: &mut HashMap<u16, u16>,
  used_gids: &mut IndexSet<u16>,
) -> ShaperResult<Vec<ShapingResult>> {
  let (hb_shaper, shape_plan, mut buffer) = shaper.build()?;
  buffer.push_str(text);

  let shape_result = hb_shaper.shape_with_plan(&shape_plan, buffer, &shaper.features);
  let glyph_positions = shape_result.glyph_positions();
  let glyph_infos = shape_result.glyph_infos();

  let mut shaping_results = Vec::with_capacity(glyph_positions.len());

  for (glyph_position, glyph_info) in glyph_positions.iter().zip(glyph_infos) {
    let gid = glyph_info.glyph_id as u16;
    let cluster = glyph_info.cluster;
    let x_advance = glyph_position.x_advance;
    let y_advance = glyph_position.y_advance;
    let x_offset = glyph_position.x_offset;
    let y_offset = glyph_position.y_offset;

    shaping_results.push(ShapingResult {
      gid,
      cluster,
      x_advance,
      y_advance,
      x_offset,
      y_offset,
    });

    let len = gid_to_cid.len();
    gid_to_cid.entry(gid).or_insert_with(|| len as u16);
    used_gids.insert(gid);
  }

  Ok(shaping_results)
}

/// シェーピング結果から文字範囲を取得
///
/// 指定されたインデックスのシェーピング結果に対応する元のテキストの文字範囲を返します。
/// クラスタ情報を使用して、グリフが元のテキストのどの部分に対応するかを特定します。
/// 合字や結合文字の場合、複数の文字が1つのグリフに対応することがあります。
///
/// # 引数
///
/// * `line` - 元のテキスト行
/// * `shape_results` - シェーピング結果のスライス
/// * `current_index` - 現在のシェーピング結果のインデックス
///
/// # 戻り値
///
/// 文字範囲を表す`Range<usize>`を返します（バイト単位のインデックス）。
fn get_char_range(line: &str, shape_results: &[ShapingResult], current_index: usize) -> std::ops::Range<usize> {
  let start = shape_results[current_index].cluster as usize;
  let end = shape_results.get(current_index + 1).map(|sr| sr.cluster as usize).unwrap_or(line.len());
  start..end
}
