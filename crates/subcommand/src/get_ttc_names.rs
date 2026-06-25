//! TrueType Collection (TTC) ファイルの構成確認サブコマンド
//!
//! TTC ファイル内に含まれるすべてのフォントの名前情報を取得・表示します。
//! name テーブルのレコードを解析し、テキスト形式で標準出力に出力します。
//!
//! ## TTC ファイルとは
//!
//! TrueType Collection (TTC) は、複数のフォントを 1 ファイルにまとめたファイル形式です。
//! 特に日本語フォントや中国語・韓国語フォントで一般的です。
//!
//! 利点：
//! - **ファイルサイズ最適化** - 共通部分の重複排除（例：CFF またはグリフテーブル）
//! - **複数ウェイト統合** - Regular、Bold などを 1 ファイルで提供
//! - **言語別バリアント** - 複数の言語体系の字体を 1 ファイルで管理
//!
//! 例：`SourceHanCodeJP.ttc` には、複数の言語バリアント（中国語、日本語、韓国語など）が含まれます。
//!
//! ## OpenType name テーブルとは
//!
//! すべてのフォントは、複数のプラットフォーム（Windows、macOS など）向けの
//! 名前情報を持つ name テーブルを含みます。
//!
//! 各名前レコードは以下の情報から構成されます：
//!
//! - **Platform ID** - プラットフォーム種別
//!   - `1` = Macintosh（macOS 互換）
//!   - `3` = Windows（最も一般的）
//!
//! - **Encoding ID** - テキストエンコーディング
//!   - `1` = Unicode BMP（Platform ID 1 の場合）
//!   - `10` = Unicode full repertoire（Platform ID 3 の場合）
//!
//! - **Language ID** - 言語コード（プラットフォーム依存）
//!
//! - **Name ID** - 名前種別（OpenType 標準）
//!   - `1` = Typeface Family name（ファミリー名）
//!   - `2` = Typeface Subfamily name（太字、イタリック情報）
//!   - `4` = Full font name（完全なフォント名）
//!   - `6` = PostScript name（PDF で使用する一意の名前）
//!
//! ## 出力情報
//!
//! このコマンドは、各フォント × 各名前レコードの組み合わせごとに 1 行出力します。
//! TTC に複数フォント、複数言語が含まれている場合、大量の行が出力されます。
//!
//! ## 使用例
//!
//! ```bash
//! # TTC ファイルの全フォント名を表示（出力が多い場合は less などでフィルター）
//! seiran ttc-names fonts/SourceHanCodeJP.ttc | less
//!
//! # 特定のフォント名（Name ID 4）のみを検索
//! seiran ttc-names fonts/SourceHanCodeJP.ttc | grep "Name ID NameRecord(4)"
//! ```

use std::{fs, path::Path};

use miette::{Diagnostic, IntoDiagnostic};
use read_fonts::{FontRef, TableProvider};
use thiserror::Error;
use tracing::info;

/// TTC ファイル情報取得時のエラー型
#[derive(Debug, Error, Diagnostic)]
enum TtcNamesError {
  /// TTC ファイルの読み込みに失敗した場合
  #[error("TTC ファイルの読み込みに失敗しました: {path}")]
  #[diagnostic(code(ttc_names::read_file), help("ファイルのパスと読み取り権限を確認してください。"))]
  ReadFile {
    /// ファイルパス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },
}

/// TTC ファイル内のすべてのフォント名情報を取得・表示
///
/// TrueType Collection (TTC) ファイルに含まれるすべてのフォントについて、
/// name テーブルのレコード情報を解析して、テキスト形式で標準出力に表示します。
///
/// 各フォントごとに、プラットフォーム ID、エンコーディング ID、言語 ID、
/// Name ID、および名前文字列を行ごとに出力します。
///
/// # Arguments
///
/// * `file_path` - 検査対象の TTC ファイルのパス
///   (例：`SourceHanCodeJP.ttc`、`NotoSerifJP.ttc`)
///
/// # Returns
///
/// - `Ok(())` - すべてのフォント名情報の表示に成功
/// - `Err(miette::Diagnostic)` - ファイル読み込みまたは解析失敗
///
/// # Errors
///
/// 以下のいずれかで失敗します：
///
/// - **ファイル I/O エラー** - TTC ファイルが見つからない、読み込み権限がない
/// - **フォント形式エラー** - ファイルが TTC 形式でない、形式が不正
/// - **テーブル欠落エラー** - name テーブルが見つからない、フォント構造が破損している
/// - **解析エラー** - 名前レコードの解析に失敗
///
/// # Output Format
///
/// 標準出力に以下の形式でテキストを出力します（1 フォント = 複数行）：
///
/// ```text
/// Font Index {index}: Platform ID {id}, Encoding ID {id}, Language ID {id}, Name ID {id}: {name}
/// ```
///
/// ## 出力例
///
/// ```text
/// Font Index 0: Platform ID Macintosh, Encoding ID Unicode, Language ID English, Name ID Family: "Source Han Code JP"
/// Font Index 0: Platform ID Macintosh, Encoding ID Unicode, Language ID English, Name ID Subfamily: "Regular"
/// Font Index 0: Platform ID Windows, Encoding ID UnicodeFullRepertoire, Language ID 0x0409, Name ID Family: "Source Han Code JP"
/// ```
///
/// # Example
///
/// ```ignore
/// seiran ttc-names fonts/SourceHanCodeJP.ttc
/// ```
pub fn get_ttc_names(file_path: &Path) -> miette::Result<()> {
  // TTC ファイルパスをログに記録
  info!(ttc_path = %file_path.display(), "TTC ファイルを読み込みます");

  // TTC ファイルをすべてメモリに読み込み
  let data = fs::read(file_path).map_err(|source| TtcNamesError::ReadFile {
    path: file_path.display().to_string(),
    source,
  })?;

  // TTC に含まれるすべてのフォントを反復
  // FontRef::fonts() は TTC 内の複数フォントを自動検出
  let fonts = FontRef::fonts(&data);
  for (index, font) in fonts.enumerate() {
    // 各フォント参照を解析
    let font = font.into_diagnostic()?;

    // name テーブル（フォント名情報を含む）を取得
    let names = font.name().into_diagnostic()?;

    // name テーブル内のすべてのレコードを反復
    // 各レコードは異なるプラットフォーム、言語、Name ID の組み合わせ
    for name_record in names.name_record() {
      let platform_id = name_record.platform_id();
      let encording_id = name_record.encoding_id();
      let language_id = name_record.language_id();
      let name_id = name_record.name_id();

      // 名前レコードの文字列を取得
      let name = name_record.string(names.string_data());

      // フォント情報をテキスト形式で標準出力に出力（各フォントごとに複数行）
      println!(
        "Font Index {index}: Platform ID {platform_id:?}, Encoding ID {encording_id:?}, Language ID {language_id:?}, Name ID {name_id:?}: {name:?}",
      );
    }
  }

  return Ok(());
}
