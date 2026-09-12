//! バリアブルフォントの fvar 軸と名前付きインスタンスを表示するサブコマンド

use std::{fs, io::Write, path::Path};

use miette::Diagnostic;
use read_fonts::{
  FontRef, ReadError, TableProvider, TopLevelTable,
  tables::{
    fvar::{Fvar, InstanceRecord, VariationAxisRecord},
    name::Name,
  },
  types::NameId,
};
use thiserror::Error;
use tracing::info;

use crate::subcommand::listing;

/// fvar を持たないフォントに対して出す 1 行。
const NOT_VARIABLE: &str = "The font is not a variable font.";

/// バリアブルフォント軸情報取得時のエラー型
#[derive(Debug, Error, Diagnostic)]
enum VariationAxesError {
  /// フォントファイルの読み込みに失敗した場合
  #[error("フォントファイルの読み込みに失敗しました: {path}")]
  #[diagnostic(
    code(cli::variation_axes::read_file),
    help("フォントファイルのパスと読み取り権限を確認してください。")
  )]
  ReadFile {
    /// ファイルパス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },

  /// フォント解析に失敗した場合
  #[error("インデックス {font_index} のフォント解析に失敗しました: {path}")]
  #[diagnostic(
    code(cli::variation_axes::font_parse),
    help(
      "ファイルが有効なフォントファイル (TTF/OTF/TTC/OTC) であることを確認してください。TTC の場合は --font-index を確認してください。"
    )
  )]
  FontParse {
    /// ファイルパス
    path: String,
    /// フォントインデックス
    font_index: u32,
    /// 元の解析エラー
    #[source]
    source: ReadError,
  },

  /// fvar テーブルがあるのに読めない場合
  #[error("fvar テーブルを読めませんでした: {path}")]
  #[diagnostic(
    code(cli::variation_axes::fvar),
    help("fvar テーブルが破損しています。フォントファイルを検証してください。")
  )]
  Fvar {
    /// ファイルパス
    path: String,
    /// 元の解析エラー
    #[source]
    source: ReadError,
  },

  /// 宣言された件数のレコードが fvar に収まっていない場合
  #[error("fvar の{records}レコードは {declared} 件と宣言されていますが、{readable} 件しか読めません: {path}")]
  #[diagnostic(
    code(cli::variation_axes::truncated_records),
    help(
      "fvar テーブルが途中で切れているか、ヘッダの件数・サイズが破損しています。フォントファイルを検証してください。"
    )
  )]
  TruncatedRecords {
    /// ファイルパス
    path: String,
    /// レコードの種類（「軸」または「インスタンス」）
    records: &'static str,
    /// ヘッダが宣言する件数
    declared: u16,
    /// 実際に読めた件数
    readable: usize,
  },

  /// `InstanceRecord` を読めない場合
  #[error("fvar のインデックス {instance_index} の InstanceRecord を読めませんでした: {path}")]
  #[diagnostic(
    code(cli::variation_axes::instance),
    help("fvar テーブルの instanceSize かインスタンス配列が破損しています。フォントファイルを検証してください。")
  )]
  Instance {
    /// ファイルパス
    path: String,
    /// インスタンスの添字（0 始まり）
    instance_index: usize,
    /// 元の解析エラー
    #[source]
    source: ReadError,
  },

  /// name テーブルを読めない場合
  #[error("name テーブルを読めませんでした: {path}")]
  #[diagnostic(
    code(cli::variation_axes::name),
    help("name テーブルが欠落しているか破損しています。フォントファイルを検証してください。")
  )]
  Name {
    /// ファイルパス
    path: String,
    /// 元の解析エラー
    #[source]
    source: ReadError,
  },
}

/// fvar から読み出した軸と名前付きインスタンス（どちらも宣言された件数どおり）。
struct FvarRecords<'a> {
  /// 軸レコード
  axes: &'a [VariationAxisRecord],
  /// 名前付きインスタンス
  instances: Vec<InstanceRecord<'a>>,
}

/// 指定フォントの軸情報と名前付きインスタンスの一覧を `out` へ書く。
///
/// fvar を持たないフォントは「可変フォントではない」旨の 1 行を出して成功する。
///
/// # Errors
///
/// ファイルの読み込み、フォント・fvar・name テーブルの解析、一覧の書き込み（受け手の終了を除く）に
/// 失敗した場合にエラーを返す。
pub(crate) fn variation_axes(font_path: &Path, font_index: u32, out: &mut impl Write) -> miette::Result<()> {
  let font_bytes = fs::read(font_path).map_err(|source| {
    return VariationAxesError::ReadFile {
      path: font_path.display().to_string(),
      source,
    };
  })?;
  info!(font_path = %font_path.display(), font_index, "バリエーション軸を調べるフォントファイルを読込");

  let lines = listing_lines(&font_bytes, font_index, font_path)?;
  listing::emit(&lines, out)?;
  return Ok(());
}

/// 軸 1 本 1 行、続いて名前付きインスタンス 1 件 1 行の一覧を組み立てる。
fn listing_lines(font_bytes: &[u8], font_index: u32, font_path: &Path) -> Result<Vec<String>, VariationAxesError> {
  let path = || return font_path.display().to_string();
  let font_ref = FontRef::from_index(font_bytes, font_index).map_err(|source| {
    return VariationAxesError::FontParse {
      path: path(),
      font_index,
      source,
    };
  })?;

  // 「fvar が無い」はテーブルディレクトリにレコードが無いことで判定する。`fvar()` の `TableIsMissing` は、
  // レコードはあるがオフセット + 長さがファイルからはみ出す破損フォントでも返るので、エラーの種類では
  // 「無い」と「壊れている」を区別できない
  let has_fvar = font_ref.table_directory.table_records().iter().any(|record| return record.tag() == Fvar::TAG);
  if !has_fvar {
    return Ok(vec![NOT_VARIABLE.to_owned()]);
  }
  let fvar = font_ref.fvar().map_err(|source| {
    return VariationAxesError::Fvar {
      path: path(),
      source,
    };
  })?;
  let records = fvar_records(&fvar, font_path)?;
  let name = font_ref.name().map_err(|source| {
    return VariationAxesError::Name {
      path: path(),
      source,
    };
  })?;

  let mut lines = Vec::new();
  for axis in records.axes {
    let axis_tag = axis.axis_tag();
    let min_value = axis.min_value();
    let default_value = axis.default_value();
    let max_value = axis.max_value();
    lines.push(format!("Axis: {axis_tag}, Min: {min_value}, Default: {default_value}, Max: {max_value}"));
  }
  for instance in &records.instances {
    let instance_name = subfamily_name(&name, instance.subfamily_name_id);
    let coordinates = instance.coordinates;
    lines.push(format!("{instance_name}: {coordinates:?}"));
  }
  return Ok(lines);
}

/// fvar の軸とインスタンスを、宣言された件数どおり読めたことを確かめて取り出す。
///
/// # Errors
///
/// 配列を解決できない、宣言件数ぶん読めない、または `InstanceRecord` を読めない場合にエラーを返す。
fn fvar_records<'a>(fvar: &Fvar<'a>, font_path: &Path) -> Result<FvarRecords<'a>, VariationAxesError> {
  let path = || return font_path.display().to_string();
  // read-fonts は宣言件数ぶんの範囲がテーブルからはみ出すと、エラーではなく空の配列を返す。件数を
  // 突き合わせないと「途中で切れた fvar」を「軸・インスタンスの無いフォント」として一覧にしてしまう
  let axes = fvar.axes().map_err(|source| {
    return VariationAxesError::Fvar {
      path: path(),
      source,
    };
  })?;
  ensure_record_count(font_path, "軸", fvar.axis_count(), axes.len())?;
  let instance_array = fvar.instances().map_err(|source| {
    return VariationAxesError::Fvar {
      path: path(),
      source,
    };
  })?;
  ensure_record_count(font_path, "インスタンス", fvar.instance_count(), instance_array.len())?;

  // instanceSize が軸数に対して小さいと、要素の範囲が次の要素へ食い込み、最後の要素の読み取りが必ず
  // 配列の外へ出て `Err` になる。`flatten` で落とさず `?` で上げれば、その破損も診断になる
  let instances = instance_array
    .iter()
    .enumerate()
    .map(|(instance_index, instance)| {
      return instance.map_err(|source| {
        return VariationAxesError::Instance {
          path: path(),
          instance_index,
          source,
        };
      });
    })
    .collect::<Result<Vec<_>, _>>()?;
  return Ok(FvarRecords { axes, instances });
}

/// 宣言件数と実際に読めた件数が一致することを確かめる。
///
/// # Errors
///
/// 一致しないとき [`VariationAxesError::TruncatedRecords`] を返す。
fn ensure_record_count(
  font_path: &Path,
  records: &'static str,
  declared: u16,
  readable: usize,
) -> Result<(), VariationAxesError> {
  if readable == usize::from(declared) {
    return Ok(());
  }
  return Err(VariationAxesError::TruncatedRecords {
    path: font_path.display().to_string(),
    records,
    declared,
    readable,
  });
}

/// インスタンスの表示名を name テーブルから引く。
///
/// 該当するレコードが無ければ `NameID(n)`。レコードはあるが文字列を読めなければ、その旨のマーカーを
/// 添える — 読めなかったことを「名前が無い」と同じ表示に畳まない（`script-langs` のマーカー行と同じ、
/// 表示用の解決はレコード単位で閉じて続行する扱い）。
fn subfamily_name(name: &Name<'_>, name_id: NameId) -> String {
  let Some(record) = name.name_record().iter().find(|record| return record.name_id() == name_id) else {
    return format!("NameID({name_id})");
  };
  return match record.string(name.string_data()) {
    Ok(text) => text.to_string(),
    Err(source) => format!("NameID({name_id}) (name 文字列の読み取りに失敗しました: {source})"),
  };
}

#[cfg(test)]
mod tests {
  use std::path::Path;

  use read_fonts::{
    FontData, FontRead,
    tables::{fvar::Fvar, name::Name},
    types::NameId,
  };

  use super::{VariationAxesError, fvar_records, subfamily_name};

  /// テストで診断に載せるパス。
  const FONT_PATH: &str = "test.ttf";

  /// wght 軸 1 本（100〜900、既定 400）の VariationAxisRecord（20 バイト）。
  fn axis_record() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"wght"); // axisTag
    bytes.extend_from_slice(&(100i32 << 16).to_be_bytes()); // minValue（Fixed）
    bytes.extend_from_slice(&(400i32 << 16).to_be_bytes()); // defaultValue（Fixed）
    bytes.extend_from_slice(&(900i32 << 16).to_be_bytes()); // maxValue（Fixed）
    bytes.extend_from_slice(&0u16.to_be_bytes()); // flags
    bytes.extend_from_slice(&256u16.to_be_bytes()); // axisNameID
    return bytes;
  }

  /// 軸 1 本ぶんの InstanceRecord（postScriptNameID なし・8 バイト）。
  fn instance_record(subfamily_name_id: u16, weight: i32) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&subfamily_name_id.to_be_bytes()); // subfamilyNameID
    bytes.extend_from_slice(&0u16.to_be_bytes()); // flags
    bytes.extend_from_slice(&(weight << 16).to_be_bytes()); // coordinates[0]（Fixed）
    return bytes;
  }

  /// fvar テーブルのバイト列を組む。
  ///
  /// ヘッダの宣言値と、実際に後ろへ置くレコード（`body`）を別々に渡すので、宣言と中身の食い違いを作れる。
  fn fvar_bytes(axis_count: u16, instance_count: u16, instance_size: u16, body: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1u16.to_be_bytes()); // majorVersion
    bytes.extend_from_slice(&0u16.to_be_bytes()); // minorVersion
    bytes.extend_from_slice(&16u16.to_be_bytes()); // axesArrayOffset（ヘッダ直後）
    bytes.extend_from_slice(&2u16.to_be_bytes()); // reserved
    bytes.extend_from_slice(&axis_count.to_be_bytes()); // axisCount
    bytes.extend_from_slice(&20u16.to_be_bytes()); // axisSize
    bytes.extend_from_slice(&instance_count.to_be_bytes()); // instanceCount
    bytes.extend_from_slice(&instance_size.to_be_bytes()); // instanceSize
    for record in body {
      bytes.extend_from_slice(record);
    }
    return bytes;
  }

  /// 組んだバイト列から fvar を読み、`fvar_records` の結果を返す。
  #[expect(
    clippy::unwrap_in_result,
    reason = "expect するのは組み立てた 16 バイトのヘッダで、Fvar::read が失敗し得ない — テスト自体の前提が壊れているとき用"
  )]
  fn run_fvar_records(bytes: &[u8]) -> Result<(usize, Vec<NameId>), VariationAxesError> {
    let fvar = Fvar::read(FontData::new(bytes)).expect("16 バイトのヘッダがあれば Fvar 自体は読める");
    return fvar_records(&fvar, Path::new(FONT_PATH)).map(|records| {
      return (
        records.axes.len(),
        records.instances.iter().map(|instance| return instance.subfamily_name_id).collect(),
      );
    });
  }

  #[test]
  fn well_formed_fvar_yields_all_records() {
    let bytes = fvar_bytes(
      1,
      2,
      8,
      &[
        axis_record(),
        instance_record(257, 100),
        instance_record(258, 900),
      ],
    );

    let (axis_count, instance_names) = run_fvar_records(&bytes).expect("正しい fvar は読める");

    assert_eq!(axis_count, 1);
    assert_eq!(instance_names, vec![NameId::new(257), NameId::new(258)]);
  }

  #[test]
  fn unresolvable_axes_array_is_an_fvar_error() {
    // Arrange — axesArrayOffset をテーブル外へ向ける
    let mut bytes = fvar_bytes(1, 0, 8, &[axis_record()]);
    bytes[4..6].copy_from_slice(&0xffffu16.to_be_bytes());

    // Act
    let error = run_fvar_records(&bytes).expect_err("配列を解決できない fvar は失敗する");

    // Assert
    assert!(
      matches!(&error, VariationAxesError::Fvar { path, .. } if path == FONT_PATH),
      "対象パス付きの fvar 破損: {error:?}"
    );
  }

  #[test]
  fn truncated_axes_are_reported_instead_of_listing_none() {
    let bytes = fvar_bytes(2, 0, 8, &[axis_record()]);

    let error = run_fvar_records(&bytes).expect_err("宣言より少ない軸は失敗する");

    assert!(
      matches!(
        error,
        VariationAxesError::TruncatedRecords {
          records: "軸",
          declared: 2,
          readable: 0,
          ..
        }
      ),
      "read-fonts が空配列に畳んだ切り詰めを検出する: {error:?}"
    );
  }

  #[test]
  fn truncated_instances_are_reported_instead_of_listing_none() {
    let bytes = fvar_bytes(1, 3, 8, &[axis_record(), instance_record(257, 100)]);

    let error = run_fvar_records(&bytes).expect_err("宣言より少ないインスタンスは失敗する");

    assert!(
      matches!(
        error,
        VariationAxesError::TruncatedRecords {
          records: "インスタンス",
          declared: 3,
          readable: 0,
          ..
        }
      ),
      "read-fonts が空配列に畳んだ切り詰めを検出する: {error:?}"
    );
  }

  #[test]
  fn unreadable_instance_record_is_not_dropped() {
    // Arrange — instanceSize 2 は軸 1 本の InstanceRecord（8 バイト）に足りず、要素の読み取りが Err になる
    let bytes = fvar_bytes(1, 2, 2, &[axis_record(), vec![0; 4]]);

    // Act
    let error = run_fvar_records(&bytes).expect_err("読めない InstanceRecord は失敗する");

    // Assert
    assert!(
      matches!(
        error,
        VariationAxesError::Instance {
          instance_index: 0,
          ..
        }
      ),
      "Err 要素を黙って落とさない: {error:?}"
    );
  }

  /// nameID 256 のレコード 1 件だけを持つ name テーブル（format 0）。
  ///
  /// `string_offset` は文字列領域先頭からの位置で、0 なら "Th"（UTF-16BE）を指す。
  fn name_bytes(string_offset: u16) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0u16.to_be_bytes()); // version
    bytes.extend_from_slice(&1u16.to_be_bytes()); // count
    bytes.extend_from_slice(&18u16.to_be_bytes()); // storageOffset（ヘッダ 6 + レコード 12）
    bytes.extend_from_slice(&3u16.to_be_bytes()); // platformID（Windows）
    bytes.extend_from_slice(&1u16.to_be_bytes()); // encodingID（Unicode BMP）
    bytes.extend_from_slice(&0x0409u16.to_be_bytes()); // languageID（en-US）
    bytes.extend_from_slice(&256u16.to_be_bytes()); // nameID
    bytes.extend_from_slice(&4u16.to_be_bytes()); // length
    bytes.extend_from_slice(&string_offset.to_be_bytes()); // stringOffset
    bytes.extend_from_slice(&[0, b'T', 0, b'h']); // 文字列領域
    return bytes;
  }

  #[test]
  fn instance_name_is_resolved_from_the_name_table() {
    let bytes = name_bytes(0);
    let name = Name::read(FontData::new(&bytes)).expect("name テーブルは読める");

    assert_eq!(subfamily_name(&name, NameId::new(256)), "Th");
  }

  #[test]
  fn absent_name_record_falls_back_to_the_name_id() {
    let bytes = name_bytes(0);
    let name = Name::read(FontData::new(&bytes)).expect("name テーブルは読める");

    assert_eq!(subfamily_name(&name, NameId::new(257)), format!("NameID({})", NameId::new(257)));
  }

  #[test]
  fn unreadable_name_string_is_marked_not_folded_into_the_name_id() {
    // Arrange — 文字列の位置を文字列領域の外へ向ける
    let bytes = name_bytes(0x00ff);
    let name = Name::read(FontData::new(&bytes)).expect("name テーブルの枠は読める");

    // Act
    let shown = subfamily_name(&name, NameId::new(256));

    // Assert
    assert!(
      shown.starts_with(&format!("NameID({}) (name 文字列の読み取りに失敗しました:", NameId::new(256))),
      "「レコードが無い」と同じ表示に畳まない: {shown}"
    );
  }
}
