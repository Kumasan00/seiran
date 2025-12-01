//! PDFテキスト生成アプリケーション
//!
//! このアプリケーションは、テキストファイルを読み込み、
//! 指定されたフォントを使用してPDFドキュメントを生成します。
//! フォントのサブセット化、テキストシェーピング、グリフマッピングを処理します。

use std::{fs, io, path::Path, result};

use font::{
  self,
  font_context::{FontContext, FontContexts, create_font_subset},
  font_data::{FontDatas, analyze_subset_font},
};
use pdf_writer::{Content, Finish, Name, Str};
use read_config_file::Config;
use stypes::{GlyphMapping, GlyphMappings};
use ttf_parser::Face;

// 定数

/// .notdef グリフのグリフID
const NOTDEF_GID: u16 = 0;

/// アプリケーションのメインエントリーポイント
///
/// 以下の処理を実行します：
/// 1. コマンドライン引数の解析
/// 2. 設定ファイルの読み込み
/// 3. 入力テキストファイルの読み込み
/// 4. フォントの初期化とテキスト処理
/// 5. フォントサブセットの作成
/// 6. PDF生成
///
/// # エラー
///
/// ファイルI/O、フォント処理、PDF生成のいずれかで問題が発生した場合にエラーを返します。
fn main() -> Result<(), Box<dyn std::error::Error>> {
  let cli_args = cli::parse_arg();

  match cli_args.command {
    cli::Command::Build { text_file_path } => {
      build_pdf(&text_file_path)?;
    }
    cli::Command::TtcNames { ttc_file_path } => {
      get_ttc_names(&ttc_file_path)?;
    }
  }

  Ok(())
}

/// PDFを生成する
///
/// 設定ファイルとテキストファイルを読み込み、フォント処理を行い、
/// PDFドキュメントを生成します。
///
/// # 引数
///
/// * `file_path` - 入力テキストファイルのパス
///
/// # 戻り値
///
/// 成功した場合は`Ok(())`を返します。
///
/// # エラー
///
/// ファイル読み込み、フォント処理、PDF生成のいずれかで失敗した場合。
fn build_pdf<P: AsRef<Path>>(file_path: P) -> Result<(), Box<dyn std::error::Error>> {
  let config = read_config_file::read_config_file()?;
  println!("Config loaded: {:?}", config);

  let text_lines = read_file::read_file(file_path)?;
  let mut font_contexts = FontContexts::new(&config)?;
  let mut glyph_mappings = GlyphMappings::new();

  let pdf_content = process_text_lines(
    text_lines,
    &mut font_contexts.main_font_context,
    &mut glyph_mappings.main_font,
    &config,
  )?;

  let subset_bytes = create_font_subset(&font_contexts, &glyph_mappings, &config)?;
  println!("Subset font: {} bytes", subset_bytes.main_font_subset.len());

  let font_datas = analyze_subset_font(&subset_bytes, &font_contexts)?;

  insert_notdef_advance_widths(&mut glyph_mappings, &font_datas);

  pdf_gen::pdf_gen(
    &subset_bytes,
    &font_datas,
    &glyph_mappings,
    pdf_content,
    &config,
  )?;

  println!("PDF generated");
  Ok(())
}

/// 全フォントに.notdefグリフのadvance widthを挿入
///
/// # 引数
///
/// * `glyph_mappings` - グリフマッピング情報
/// * `font_datas` - フォントデータ情報
fn insert_notdef_advance_widths(glyph_mappings: &mut GlyphMappings, font_datas: &FontDatas) {
  glyph_mappings
    .main_font
    .advance_widths
    .insert(NOTDEF_GID, font_datas.main_font_data.upem);
  glyph_mappings
    .math_font
    .advance_widths
    .insert(NOTDEF_GID, font_datas.math_font_data.upem);
  glyph_mappings
    .sans_font
    .advance_widths
    .insert(NOTDEF_GID, font_datas.sans_font_data.upem);
  glyph_mappings
    .main_japanese_font
    .advance_widths
    .insert(NOTDEF_GID, font_datas.main_japanese_font_data.upem);
  glyph_mappings
    .sans_japanese_font
    .advance_widths
    .insert(NOTDEF_GID, font_datas.sans_japanese_font_data.upem);
}

/// テキストの各行を処理してPDFコンテンツストリームを生成
///
/// # 引数
///
/// * `lines` - 処理するテキスト行のイテレータ
/// * `font_ctx` - フォントコンテキスト
/// * `mapping` - グリフマッピング情報
/// * `config` - アプリケーション設定
///
/// # 戻り値
///
/// PDFコンテンツストリームを返します。
///
/// # エラー
///
/// 行の処理中にエラーが発生した場合にエラーを返します。
fn process_text_lines(
  text_lines: io::Lines<io::BufReader<fs::File>>,
  main_font_context: &mut FontContext,
  glyph_mapping: &mut GlyphMapping,
  config: &Config,
) -> Result<Content, Box<dyn std::error::Error>> {
  let units_per_em = main_font_context.ttf_face.units_per_em() as f32;
  let mut pdf_content = Content::new();
  pdf_content.begin_text();
  pdf_content.set_font(
    Name(config.main_font.font_name.as_bytes()),
    config.pdf.font_size,
  );
  pdf_content.next_line(
    config.pdf.margin_left,
    config.pdf.height - config.pdf.margin_top,
  );

  for (line_num, line) in text_lines.enumerate() {
    let text_line = line?;
    println!("Line {}: {}", line_num + 1, text_line);

    process_single_line(
      &text_line,
      main_font_context,
      units_per_em,
      glyph_mapping,
      &mut pdf_content,
    )?;

    let line_height_factor = config.pdf.line_height_factor;
    pdf_content.next_line(0.0, -config.pdf.font_size * line_height_factor);
  }

  pdf_content.end_text();
  Ok(pdf_content)
}

/// 単一行のテキストを処理してコンテンツストリームに追加
///
/// テキストシェーピングを実行し、グリフIDとCIDのマッピングを更新し、
/// 位置調整を行いながらコンテンツストリームに書き込みます。
///
/// # 引数
///
/// * `line` - 処理するテキスト行
/// * `font_ctx` - フォントコンテキスト
/// * `upem` - フォントのユニット/em値
/// * `mapping` - グリフマッピング情報
/// * `content` - PDFコンテンツストリーム
///
/// # エラー
///
/// シェーピングまたはコンテンツ書き込み中にエラーが発生した場合にエラーを返します。
fn process_single_line(
  text_line: &str,
  main_font_context: &mut FontContext,
  units_per_em: f32,
  glyph_mapping: &mut GlyphMapping,
  pdf_content: &mut Content,
) -> Result<(), Box<dyn std::error::Error>> {
  let mut position_text = pdf_content.show_positioned();
  let mut items = position_text.items();

  let shape_results = text::shaping(
    text_line,
    &mut main_font_context.hb_font,
    &mut glyph_mapping.gid_to_cid,
    &mut glyph_mapping.used_gids,
  );

  let mut text_buffer = Vec::new();

  for (glyph_index, shape_result) in shape_results.iter().enumerate() {
    let gid = shape_result.gid;
    let Some(&cid) = glyph_mapping.gid_to_cid.get(&gid) else {
      let fallback_cid = NOTDEF_GID;
      text_buffer.push((fallback_cid >> 8) as u8);
      text_buffer.push((fallback_cid & 0xFF) as u8);
      continue;
    };

    let advance_width = main_font_context.get_glyph_advance(gid) * 1000.0 / units_per_em;
    glyph_mapping
      .advance_widths
      .entry(cid)
      .or_insert(advance_width);
    let shape_advance = shape_result.x_advance as f32 * 1000.0 / units_per_em;

    text_buffer.push((cid >> 8) as u8);
    text_buffer.push((cid & 0xFF) as u8);

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

  Ok(())
}

/// シェーピング結果から文字範囲を取得
///
/// 指定されたインデックスのシェーピング結果に対応する
/// 元のテキストの文字範囲を返します。
///
/// # 引数
///
/// * `line` - 元のテキスト行
/// * `shape_results` - シェーピング結果のスライス
/// * `current_index` - 現在のシェーピング結果のインデックス
///
/// # 戻り値
///
/// 文字範囲を表す`Range<usize>`を返します。
fn get_char_range(
  line: &str,
  shape_results: &[text::ShapingResult],
  current_index: usize,
) -> std::ops::Range<usize> {
  let start = shape_results[current_index].cluster as usize;
  let end = shape_results
    .get(current_index + 1)
    .map(|sr| sr.cluster as usize)
    .unwrap_or(line.len());
  start..end
}

/// TTCファイルから各フォントの名前情報を取得
///
/// TrueTypeコレクション(TTC)ファイルに含まれる全てのフォントの
/// nameテーブル情報を出力します。
///
/// # 引数
///
/// * `file_path` - TTCファイルのパス
///
/// # 戻り値
///
/// 成功した場合は`Ok(())`を返します。
///
/// # エラー
///
/// ファイル読み込みまたはフォント解析に失敗した場合。
fn get_ttc_names<P: AsRef<Path>>(file_path: P) -> result::Result<(), Box<dyn std::error::Error>> {
  let font_data = fs::read(file_path)?;
  let font_count = ttf_parser::fonts_in_collection(&font_data).unwrap();
  println!("Number of fonts in TTC: {}", font_count);
  for font_index in 0..font_count {
    println!("\nFont index: {}\n", font_index);
    let face = Face::parse(&font_data, font_index)?;
    // let name = font::extract_font_name(&face)?;
    let names = face.names();
    for name_entry in names {
      let platform_id = name_entry.platform_id;
      println!(
        "Platform ID {:?}: Name ID {}: {:?}",
        platform_id,
        name_entry.name_id,
        name_entry.to_string()
      );
    }
  }
  Ok(())
}
