//! フォント調査の前段 — フォントファイルの読込と、`--font-index` による face の選択
//!
//! 3 つのフォント調査サブコマンドはこの前段を共有し、一覧の行を作る段だけをそれぞれ持つ。前段の失敗の
//! 診断は code の第 2 階層（サブコマンド名）と文言がサブコマンドごとに違い、どちらも利用者に見えるので
//! 変えない。そのため失敗型はどのサブコマンドの前段かを持ち、code と help をこの module の `Diagnostic`
//! impl の対応表 1 箇所から引く（miette の derive の `code(...)` は静的なパスしか書けない）。
//!
//! face の選択（[`select_face`]）を使うのは `variation-axes` と `script-langs` だけ。`ttc-names` は
//! コレクション内の全フォントを列挙するので、`FileRef` を自分で読む。

use std::{fmt::Display, fs, io, path::Path};

use miette::Diagnostic;
use read_fonts::{FontRef, ReadError};
use thiserror::Error;

/// どのフォント調査サブコマンドの前段か（診断の code と文言を引くキー）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Inspection {
  /// `ttc-names`（コレクション内の全フォントを列挙する）
  TtcNames,
  /// `--font-index` で face を 1 つ選ぶサブコマンド
  SingleFace(FaceInspection),
}

/// `--font-index` で face を 1 つ選ぶフォント調査サブコマンド。
///
/// face 選択の失敗は `ttc-names` には起きないので、[`Inspection`] と分けて型で除く。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FaceInspection {
  /// `variation-axes`
  VariationAxes,
  /// `script-langs`
  ScriptLangs,
}

impl Inspection {
  /// 読込失敗の主メッセージで対象ファイルを呼ぶ名前。
  const fn file_noun(self) -> &'static str {
    return match self {
      Self::TtcNames => "TTC ファイル",
      Self::SingleFace(FaceInspection::VariationAxes | FaceInspection::ScriptLangs) => "フォントファイル",
    };
  }
}

/// フォントファイルを読めなかったときのエラー。
#[derive(Debug, Error)]
#[error("{}の読み込みに失敗しました: {path}", .inspection.file_noun())]
pub(super) struct ReadFontFileError {
  /// どのサブコマンドの前段か
  inspection: Inspection,
  /// ファイルパス
  path: String,
  /// 元の I/O エラー
  #[source]
  source: io::Error,
}

impl Diagnostic for ReadFontFileError {
  fn code(&self) -> Option<Box<dyn Display + '_>> {
    let code = match self.inspection {
      Inspection::TtcNames => "cli::ttc_names::read_file",
      Inspection::SingleFace(FaceInspection::VariationAxes) => "cli::variation_axes::read_file",
      Inspection::SingleFace(FaceInspection::ScriptLangs) => "cli::script_langs::read_file",
    };
    return Some(Box::new(code));
  }

  fn help(&self) -> Option<Box<dyn Display + '_>> {
    let help = match self.inspection {
      Inspection::TtcNames => "ファイルのパスと読み取り権限を確認してください。",
      Inspection::SingleFace(FaceInspection::VariationAxes | FaceInspection::ScriptLangs) => {
        "フォントファイルのパスと読み取り権限を確認してください。"
      },
    };
    return Some(Box::new(help));
  }
}

/// ファイルから指定インデックスの face を選べなかったときのエラー。
#[derive(Debug, Error)]
#[error("インデックス {font_index} のフォント解析に失敗しました: {path}")]
pub(super) struct SelectFaceError {
  /// どのサブコマンドの前段か
  inspection: FaceInspection,
  /// ファイルパス
  path: String,
  /// フォントインデックス（TTC の場合はコレクション内の位置）
  font_index: u32,
  /// 元の解析エラー
  #[source]
  source: ReadError,
}

impl Diagnostic for SelectFaceError {
  fn code(&self) -> Option<Box<dyn Display + '_>> {
    let code = match self.inspection {
      FaceInspection::VariationAxes => "cli::variation_axes::font_parse",
      // `_error` 接尾辞は script-langs だけの不揃いだが、利用者に見える code なので保つ（#685 のスコープ外）
      FaceInspection::ScriptLangs => "cli::script_langs::font_parse_error",
    };
    return Some(Box::new(code));
  }

  fn help(&self) -> Option<Box<dyn Display + '_>> {
    let help = match self.inspection {
      FaceInspection::VariationAxes => {
        "ファイルが有効なフォントファイル (TTF/OTF/TTC/OTC) であることを確認してください。TTC の場合は --font-index を確認してください。"
      },
      FaceInspection::ScriptLangs => {
        "ファイルが有効なフォントファイル (TTF/OTF/TTC/OTC) であることを確認してください。TTC の場合は別のインデックスを試してください。"
      },
    };
    return Some(Box::new(help));
  }
}

/// フォントファイルを読む。
///
/// # Errors
///
/// ファイルを読めなかったとき [`ReadFontFileError`] を返す。
pub(super) fn read(path: &Path, inspection: Inspection) -> Result<Vec<u8>, ReadFontFileError> {
  return fs::read(path).map_err(|source| {
    return ReadFontFileError {
      inspection,
      path: path.display().to_string(),
      source,
    };
  });
}

/// 読み込んだバイト列から `font_index` 番目の face を選ぶ（単体フォントは 0 だけが有効）。
///
/// # Errors
///
/// バイト列がフォントとして解析できない、またはインデックスがコレクションの範囲外のとき
/// [`SelectFaceError`] を返す。
pub(super) fn select_face<'a>(
  bytes: &'a [u8],
  font_index: u32,
  path: &Path,
  inspection: FaceInspection,
) -> Result<FontRef<'a>, SelectFaceError> {
  return FontRef::from_index(bytes, font_index).map_err(|source| {
    return SelectFaceError {
      inspection,
      path: path.display().to_string(),
      font_index,
      source,
    };
  });
}

#[cfg(test)]
mod tests {
  use std::{error::Error, path::Path};

  use miette::Diagnostic;

  use super::{FaceInspection, Inspection, read, select_face};

  /// 診断の code・主メッセージ・help を文字列の組にする。
  fn rendered(error: &impl Diagnostic) -> (String, String, String) {
    let code = error.code().expect("前段の失敗は code を持つ").to_string();
    let help = error.help().expect("前段の失敗は help を持つ").to_string();
    return (code, error.to_string(), help);
  }

  #[test]
  fn read_failure_keeps_each_subcommand_code_and_wording() {
    let cases = [
      (
        Inspection::TtcNames,
        "cli::ttc_names::read_file",
        "TTC ファイルの読み込みに失敗しました: no-such-dir/missing",
        "ファイルのパスと読み取り権限を確認してください。",
      ),
      (
        Inspection::SingleFace(FaceInspection::VariationAxes),
        "cli::variation_axes::read_file",
        "フォントファイルの読み込みに失敗しました: no-such-dir/missing",
        "フォントファイルのパスと読み取り権限を確認してください。",
      ),
      (
        Inspection::SingleFace(FaceInspection::ScriptLangs),
        "cli::script_langs::read_file",
        "フォントファイルの読み込みに失敗しました: no-such-dir/missing",
        "フォントファイルのパスと読み取り権限を確認してください。",
      ),
    ];

    for (inspection, code, message, help) in cases {
      let error = read(Path::new("no-such-dir/missing"), inspection).expect_err("存在しないパスは読めない");

      assert_eq!(rendered(&error), (code.to_owned(), message.to_owned(), help.to_owned()), "{inspection:?}");
      assert!(error.source().is_some(), "OS エラーは cause に残す: {inspection:?}");
    }
  }

  #[test]
  fn face_selection_failure_keeps_each_subcommand_code_and_wording() {
    let cases = [
      (
        FaceInspection::VariationAxes,
        "cli::variation_axes::font_parse",
        "ファイルが有効なフォントファイル (TTF/OTF/TTC/OTC) であることを確認してください。TTC の場合は --font-index を確認してください。",
      ),
      (
        // `_error` 接尾辞は script-langs だけの不揃いだが、利用者に見える code なので保つ（#685 のスコープ外）
        FaceInspection::ScriptLangs,
        "cli::script_langs::font_parse_error",
        "ファイルが有効なフォントファイル (TTF/OTF/TTC/OTC) であることを確認してください。TTC の場合は別のインデックスを試してください。",
      ),
    ];

    for (inspection, code, help) in cases {
      let Err(error) = select_face(b"not a font", 3, Path::new("notes.txt"), inspection) else {
        panic!("フォントでないバイト列から face は選べない: {inspection:?}");
      };

      assert_eq!(
        rendered(&error),
        (
          code.to_owned(),
          "インデックス 3 のフォント解析に失敗しました: notes.txt".to_owned(),
          help.to_owned()
        ),
        "{inspection:?}"
      );
      assert!(error.source().is_some(), "解析エラーは cause に残す: {inspection:?}");
    }
  }
}
