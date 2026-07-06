//! 処理済み・検証済み設定構造体
//!
//! このモジュールの構造体群は、TOML からデシリアライズされた後、
//! 以下の処理が完了した最終形式の設定を表します：
//!
//! - **パス解決**: 相対パスを絶対パスに、シンボリックリンクを解決
//! - **バリデーション**: すべての値が妥当な範囲内であることを確認
//! - **型変換**: スクリプト・OT 言語タグを `[u8; 4]`、BCP 47 言語を `String` に変換
//! - **正規化**: バリアブル軸、フィーチャータグを標準形式に統一
//!
//! アプリケーションはこれら処理済み設定を直接使用し、
//! バリデーションエラーの心配なく安全に処理できます。
//!
//! ## 処理済み状態の保証
//!
//! - **`Config`** - ドキュメント全体の設定（`lib.rs` で完全検証済み）
//! - **`FontConfigs`** - 19 フォント設定のコンテナ（値の正当性保証）
//! - **`FontConfig`** - 単一フォント設定（パス・値・型すべて検証済み）
//! - **`PdfConfig`** - PDF ページ設定（値の正当性保証）
//! - **`VariationAxis`** - バリアブル軸（軸値の範囲内性確認済み）
//! - **`Feature`** - OpenType フィーチャー（タグ長・値の妥当性確認済み）
//! - **`Margin`** - ページ余白（非負値・合計妥当性確認済み）
//! - **`TextDirection`** - 書字方向（`harfrust::Direction` の Invalid 以外にマップ）

use std::{path::PathBuf, str::FromStr};

use thiserror::Error;
use types::{FontMap, Length};

/// PDF 生成に必要な完全な設定情報
///
/// すべてのパス、バリデーション、型変換が完了した最終形式の設定です。
/// [`crate::read_config`] で生成され、すべての値が検証済みであることが保証されます。
///
/// アプリケーションはこの構造体から設定を読み取り、PDF 生成パイプラインに渡します。
#[derive(Debug, Clone)]
pub struct Config {
  /// ドキュメントメタデータ（title / author / date / subject）
  pub document: DocumentConfig,
  /// 出力ファイル名・ディレクトリ
  pub output: OutputConfig,
  /// PDF ページレイアウト設定（検証済み）
  pub pdf: PdfConfig,
  /// ラスタ画像のダウンサンプリング設定（検証済み）
  pub image: ImageConfig,
  /// 19 フォント種別すべての設定（検証済み）
  pub font_configs: FontConfigs,
  /// ソースファイル一覧（順次パースして 1 ドキュメントに結合、絶対パス正規化済み）
  pub sources: Vec<PathBuf>,
  /// スタイル設定ファイルへのパス（オプション、正規化済み）
  pub style_path: Option<PathBuf>,
  /// 参照設定ファイルへのパス（オプション、正規化済み）
  pub references_path: Option<PathBuf>,
}

/// PDF メタデータ
#[derive(Debug, Clone)]
pub struct DocumentConfig {
  /// ドキュメントタイトル（PDF メタデータの /Title）
  pub title: Option<String>,
  /// 著者名（PDF メタデータの /Author）
  pub author: Option<String>,
  /// 日付（ISO 8601 形式想定。PDF 出力時に必要に応じて D:YYYYMMDD 形式に変換）
  pub date: Option<String>,
  /// 主題（PDF メタデータの /Subject）
  pub subject: Option<String>,
  /// ドキュメント全体の言語（BCP 47、PDF メタデータの /Lang）
  ///
  /// 文書のロケール属性。PDF /Lang への反映とハイフネーションの言語解決に使用するほか、将来の
  /// cleveref の i18n 表示（"Figure" / "図"）・禁則処理などが参照する基準言語です。フォント単位の
  /// [`FontConfig::language`] とは独立で、こちらは文書全体の言語、フォント側はシェイピング用の
  /// 言語タグです。BCP 47 として妥当であることが検証済みです。
  pub language: Option<String>,
  /// キーワード（PDF メタデータの /Keywords）
  ///
  /// 各キーワードは非空文字列であることが検証済みです。
  pub keywords: Option<Vec<String>>,
}

/// 出力ファイル名・ディレクトリ
#[derive(Debug, Clone)]
pub struct OutputConfig {
  /// 出力ファイル名の基盤（拡張子なし。実際の PDF パスは `{output_dir}/{name}.pdf`）
  pub name: String,
  /// 出力ディレクトリの絶対パス（正規化済み）
  pub output_dir: PathBuf,
}

impl OutputConfig {
  /// `{output_dir}/{name}.pdf` の絶対パスを返す
  #[must_use]
  pub fn pdf_path(&self) -> PathBuf {
    let mut path = self.output_dir.clone();
    path.push(&self.name);
    path.set_extension("pdf");
    return path;
  }
}

/// PDF ページレイアウトの検証済み・処理済み設定
///
/// ページサイズ、余白、フォント設定など、
/// PDF 出力全体のレイアウトを制御します。
/// すべての値が正の値・非負値として検証済みです。
///
/// 出力先パスは `Config::output` を参照（`OutputConfig::pdf_path()`）。
#[derive(Debug, Clone)]
pub struct PdfConfig {
  /// ページの高さ（[`Length`]）
  /// バリデーション済み（> 0）、余白と矛盾なし
  pub height: Length,
  /// ページの幅（[`Length`]）
  /// バリデーション済み（> 0）、余白と矛盾なし
  pub width: Length,
  /// ページ余白（上下左右）
  pub margin: Margin,
  /// PDF のしおり（ブックマーク）を出力するか（既定 true）
  pub show_bookmarks: bool,
}

/// ラスタ画像のダウンサンプリング設定（検証済み）
///
/// 画像の埋め込み解像度（出力物理）を決める設定。`\image[dpi=...]` / `[downsample=...]` の
/// per-image 上書きが無いときのグローバル既定値。
#[derive(Debug, Clone, Copy)]
pub struct ImageConfig {
  /// ラスタ画像埋め込み時の最大 DPI（バリデーション済み、1〜2400）
  pub max_dpi: u32,
  /// ラスタ画像のダウンサンプリングを行うか
  pub downsample: bool,
}

/// ページ余白（上下左右）
///
/// ページ内の有効テキスト配置領域を定義します。
/// すべて非負値（>= 0）で、合計がページサイズ未満であることが保証されます。
#[derive(Debug, Clone, Copy)]
pub struct Margin {
  /// 上余白（[`Length`]）（バリデーション済み、>= 0）
  pub top: Length,
  /// 下余白（[`Length`]）（バリデーション済み、>= 0）
  pub bottom: Length,
  /// 左余白（[`Length`]）（バリデーション済み、>= 0）
  pub left: Length,
  /// 右余白（[`Length`]）（バリデーション済み、>= 0）
  pub right: Length,
}

/// 19 フォント種別すべての検証済み設定
///
/// PDF 生成に使用される 19 種類のフォント（Latin 12 + Math 1 + 日本語 6）の
/// 設定をまとめて管理します。各フォント種別は独立した `FontConfig` を持ち、
/// すべてのパス、値、型が検証済みです。
///
/// 内部的には [`FontMap<FontConfig>`] を使用しており、
/// [`iter()`](FontConfigs::iter) や [`get()`](FontConfigs::get) メソッドで
/// 効率よくアクセスできます。
pub type FontConfigs = FontMap<FontConfig>;

/// 単一フォント種別の検証済み・処理済み設定
///
/// パス正規化、バリデーション、型変換がすべて完了した状態です。
/// 以下が保証されます：
/// - `font_path` は絶対パスに正規化されている
/// - `font_index` は 0 以上の値
/// - `script`、`feature` タグは 4 文字の `[u8; 4]` に変換済み（script はユーザ指定をそのまま反映）
/// - `language` は BCP 47 として妥当な文字列、または `None`
/// - `ot_language_tag` は 4 バイトに正規化済み（3 文字指定時は末尾スペースパディング）、または `None`
/// - 値の妥当性はすべてバリデーション済み
#[derive(Debug, Clone)]
pub struct FontConfig {
  /// `PDF FontDescriptor` で使用されるフォント名
  /// 19 フォント種別間で一意である必要があります
  pub font_name: String,
  /// フォントファイルへの絶対パス（正規化済み）
  /// シンボリックリンク解決済み、ファイル存在確認済み
  pub font_path: PathBuf,
  /// TTC（TrueType Collection）ファイル内のインデックス
  /// 通常は 0。複数フォントを含むコレクションの場合は 1 以上
  pub font_index: u32,
  /// バリアブルフォント軸の設定値
  /// 値が範囲内であることはバリデーション済み
  pub variation_axes: Option<Vec<VariationAxis>>,
  /// OpenType / ISO 15924 script タグ（4 バイト、ユーザ指定の case をそのまま反映）
  ///
  /// 構造的妥当性（4 文字 ASCII アルファベット）はバリデーション済み。case は正規化せず
  /// ユーザ指定をそのまま `[u8; 4]` 化します。例：`b"latn"`（Latin）、`b"arab"`（Arabic）、
  /// `b"kana"`（Katakana/Hiragana）。
  ///
  /// このバイト列は 2 つの経路で使われます：
  /// - `font::validate_font::check_script_language_support` が GSUB/GPOS の `ScriptList` を
  ///   **バイト完全一致** で lookup し、見つからなければ `tracing::warn!` で報告します。
  ///   表記揺れ（例: `b"Latn"` vs フォントの `b"latn"`）はここで確定的に通知されます。
  /// - `font::shaper` が `harfrust::Script::from_iso15924_tag` にそのまま渡し、harfrust が
  ///   case 正規化（先頭大文字 + 残り小文字）と OT script タグ導出を行います。
  ///   したがって `b"latn"` / `b"Latn"` / `b"LATN"` は同じ Latin script として shape されます。
  ///
  /// # 注意: `b"DFLT"` 明示指定の落とし穴
  ///
  /// `b"DFLT"` を渡すと shaper 側で harfrust が `Dflt` → `dflt` と変換し、フォントの
  /// `b"DFLT"` subtable とは別物が lookup されます（`validate_font` 側ではバイト完全一致で
  /// `b"DFLT"` を見つけて「OK」と判定するが、実際の shape は意図と異なる subtable を使う）。
  /// harfrust は script 未指定時に DFLT を自動 fallback として試行するので、強制 DFLT が
  /// 欲しい場合は config で `script` を省略してください。
  pub script: Option<[u8; 4]>,
  /// BCP 47 言語タグ（harfrust の [`Language::from_str`] に渡す最終文字列）
  ///
  /// ユーザが `language` で指定した BCP 47 タグに加え、`ot_language` 指定時は
  /// 末尾に `-x-hbot<XXXX>` 予約サブタグを連結した形式となる
  /// （`language` 未指定で `ot_language` のみ指定された場合は `"und-x-hbot<XXXX>"`）。
  /// 例：`"ja-JP"`, `"und-x-hbotJAN "`, `"en-x-hbotENG "`
  ///
  /// [`Language::from_str`]: https://docs.rs/harfrust/latest/harfrust/struct.Language.html
  pub language: Option<String>,
  /// OpenType 言語システムタグ（4 バイトに正規化済み、3 文字指定時は末尾スペース）
  ///
  /// ユーザが `ot_language` を明示指定した場合のみ `Some`。GSUB/GPOS の言語サブテーブル
  /// 参照（`font` クレートの `validate_font`）に使用します。BCP 47 のみ指定時は `None` で、
  /// harfrust の内部処理に OT 言語タグ導出を委ねます。
  pub ot_language_tag: Option<[u8; 4]>,
  /// 書字方向（horizontal LTR/RTL、vertical TTB/BTB）
  ///
  /// `None` の場合は `font::shaper` 側で `harfrust::UnicodeBuffer::guess_segment_properties`
  /// に委ね、入力テキストのスクリプトから自動判定されます。`Some` の場合はその値で
  /// `harfrust::ShapePlan` を事前構築・キャッシュします。
  ///
  /// # パフォーマンス上の含意
  ///
  /// `harfrust::Shaper::shape_with_plan` は `buffer.direction == plan.direction` と
  /// `buffer.script == plan.script` の両方をアサートするため、`ShapePlan` をキャッシュするには
  /// `direction` に加えて [`FontConfig::script`] も明示指定されている必要があります。いずれかが
  /// `None` の場合（`guess_segment_properties` への委譲を含む）はキャッシュできず、shape 呼び出し
  /// ごとに plan を再構築します。
  pub direction: Option<TextDirection>,
  /// OpenType フィーチャー設定（4 バイトタグ + 値）
  /// 例："liga"（ligatures）、"smcp"（small capitals）
  pub features: Option<Vec<Feature>>,
}

/// 書字方向（`harfrust::Direction` の Invalid 以外にマップ）
///
/// `read_config` クレートは harfrust に依存しないため、harfrust の `Direction` を直接保持せず
/// 独自 enum で表現します。`font` クレートで `harfrust::Direction` への変換が行われます。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextDirection {
  /// 左から右（horizontal、ラテン文字・日本語横書き等）
  LeftToRight,
  /// 右から左（horizontal、アラビア文字・ヘブライ文字等）
  RightToLeft,
  /// 上から下（vertical、日本語縦書き等）
  TopToBottom,
  /// 下から上（vertical、極めて稀）
  BottomToTop,
}

/// [`TextDirection`] の `FromStr` 実装が受理しない方向文字列を渡されたときのエラー。
///
/// 受理する値はハイフン区切りの長形 4 種のみで、短縮形（`"ltr"` 等）や大文字・
/// スペース区切りは拒否します。`read_config` 内部の検証・変換でのみ使用し、最終的に
/// [`crate::ValidationError::Field`] の `message` へ畳み込まれます。
#[derive(Debug, Error)]
#[error(
  "direction は 'left-to-right' / 'right-to-left' / 'top-to-bottom' / 'bottom-to-top' のいずれかである必要があります"
)]
pub struct TextDirectionParseError;

impl FromStr for TextDirection {
  type Err = TextDirectionParseError;

  /// 書字方向文字列を [`TextDirection`] に変換します（検証と変換の単一情報源）。
  ///
  /// 旧 `validate_direction`（検証）と旧 `parse_text_direction`（変換）が別々に持っていた
  /// 値域の知識をこの 1 実装に集約します。これにより検証を通った値だけが変換されるという
  /// 暗黙契約に頼った `unreachable!()` を不要にします。
  fn from_str(value: &str) -> Result<Self, Self::Err> {
    return match value {
      "left-to-right" => Ok(Self::LeftToRight),
      "right-to-left" => Ok(Self::RightToLeft),
      "top-to-bottom" => Ok(Self::TopToBottom),
      "bottom-to-top" => Ok(Self::BottomToTop),
      _ => Err(TextDirectionParseError),
    };
  }
}

/// OpenType フィーチャーの設定（タグと値のペア）
///
/// フォントで有効にするシェイピング機能を指定します。
/// タグは 4 バイトの OpenType フィーチャータグです（例：`b"liga"` = ligatures、`b"smcp"` = small capitals）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Feature {
  /// OpenType フィーチャータグ（4 バイト）
  pub tag: [u8; 4],
  /// フィーチャーの値（通常は 0=無効、1=有効）
  pub value: u32,
}

/// バリアブルフォント軸の設定値
///
/// 変数フォントの特定の軸を目標値に設定します。
/// 複数の軸を組み合わせることで、フォントの特定バリエーション
/// （例：Weight=700、Width=80）を選択できます。
#[derive(Debug, Clone, Copy)]
pub struct VariationAxis {
  /// 軸名（4 バイトの OpenType 軸タグ）
  /// 例：b"wght"（weight）、b"wdth"（width）
  pub name: [u8; 4],
  /// 目標値（実数）
  /// 例：weight 軸で 700（太字）、width 軸で 80（condensed）
  pub value: f64,
}
