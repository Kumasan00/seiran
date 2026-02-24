# Seiran - PDF 生成ツール開発ガイドライン

## プロジェクト概要

Seiran は、TeX スタイルのテキストファイルから高品質な PDF を生成する Rust ベースのツールです。

### 主要コンポーネント

- **cli**: コマンドライン引数の解析
- **font**: フォント処理（読み込み、解析、サブセット化、バリアブルフォント対応）
- **parser**: テキストファイルのパース（レクサー、パーサー、評価器）
- **pdf_gen**: PDF 生成エンジン
- **read_config_file**: TOML 設定ファイルの読み込みと検証
- **text**: テキスト処理ユーティリティ
- **types**: プロジェクト全体で使用される共通型定義
- **seiran**: メインアプリケーション（エントリーポイント）

---

## コーディング規約

### 基本方針

- **言語**: Rust（edition = 2024）
- **命名規則**: Rust の標準規約に従う（snake_case、CamelCase）
- **フォーマット**: `rustfmt.toml`の設定に従う
  - インデント: 2 スペース
  - 最大行幅: 120 文字
  - use 文のグループ化: StdExternalCrate

### ドキュメンテーション

すべてのモジュール、構造体、関数に日本語のドキュメントコメントを記述してください：

```rust
//! モジュールレベルのドキュメント

/// 関数やメソッドの説明
///
/// # Arguments
///
/// * `param` - パラメータの説明
///
/// # Returns
///
/// 戻り値の説明
///
/// # Errors
///
/// エラー条件の説明
///
/// # Panics
///
/// パニック条件の説明(もしあれば)
/// 
/// # Examples
/// 
/// 必要に応じて使用例をコードブロックで示す
pub fn example(param: Type) -> Result<ReturnType, Error> {
  // 実装
}
```

### エラーハンドリング

1. **thiserror + miette による詳細なエラー診断**: すべてのカスタムエラーは`#[derive(thiserror::Error)]`と`#[derive(miette::Diagnostic)]`を併用
2. **エラー型の設計**: エラー型を使用するか、Result 型を返すのか unwrap や panic を使用するのか適切に判断
3. **詳細なエラーメッセージと診断情報**: エラーの原因を明確に示し、ユーザーに対して解決方法を提示する
4. **エラーの伝播**: `?`演算子を活用し、適切に上位にエラーを伝播させる
5. **ソースエラー**: `#[source]`属性で元のエラーを指定し、エラーチェーンを形成する

```rust
#[derive(thiserror::Error, Debug, miette::Diagnostic)]
pub enum MyError {
  #[error("Failed to read file: {path}")]
  #[diagnostic(
    code(my_error::io),
    help("ファイルのパスと読み取り権限を確認してください")
  )]
  Io {
    path: String,
    #[source]
    source: std::io::Error,
  },

  #[error("Invalid value for '{field}': {msg}")]
  #[diagnostic(code(my_error::invalid_value))]
  InvalidValue {
    field: &'static str,
    msg: String
  },
}
```

#### エラーハンドリングのベストプラクティス

- **ユーザーフレンドリーなメッセージ**: エラーメッセージは日本語で、何が起きたかを簡潔に説明
- **help 属性の活用**: `#[diagnostic(help("..."))]`でユーザーが取るべき行動を示唆
- **code 属性の設定**: `#[diagnostic(code(...))]`でエラーを一意に識別可能にする
- **型安全性**: `Box<dyn std::error::Error>`の使用はエントリーポイント（`main`）のみに限定
- **Result 型の使い分け**:
  - カスタムエラー型がある場合: `Result<T, CustomError>`
  - 複数のエラーをまとめる場合: `Result<T, Box<dyn std::error::Error>>`

### 特殊なコーディングルール

**重要**: Rust の一般的な慣習とは異なり、**関数の返り値では`return`キーワードを必ず使用してください**。

```rust
// Good: returnキーワードを使用
pub fn calculate(x: i32) -> i32 {
  let result = x * 2;
  return result;
}

// Bad: returnキーワードの省略（このプロジェクトでは非推奨）
pub fn calculate(x: i32) -> i32 {
  let result = x * 2;
  result
}
```

### コードの可読性

- 適切な空白とインデントを使用
- 複雑なロジックにはコメントで説明を追加
- 関数は単一責任の原則に従い、簡潔に保つ
- マジックナンバーは定数として定義（例: `const NOTDEF_GID: u16 = 0;`）
- 意味のある変数名と関数名を使用
- use 文は`*`を避け、明示的にインポート

---

## パフォーマンス最適化

### 並列処理

重い処理には**rayon クレート**を使用して並列化を実装してください：

```rust
use rayon::prelude::*;

// 並列イテレーション
items.par_iter().for_each(|item| {
  // 処理
});
```

### メモリ効率

- 大きなファイルの読み込みには`memmap2`を使用してメモリマップドファイルを活用
- 不要なクローンを避け、参照を適切に使用
- `IndexSet`や`HashMap`などの効率的なコレクションを活用

### PDF 生成の最適化

- フォントのサブセット化により埋め込むデータを最小限に
- グリフマッピングのキャッシュを活用
- ページごとの並列処理を検討

---

## テスト

### ユニットテストの記述

主要な機能には必ずユニットテストを追加してください：

```rust
#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_function_name() {
    // Arrange
    let input = ...;

    // Act
    let result = function_under_test(input);

    // Assert
    assert_eq!(result, expected);
  }
}
```

### テスト方針

- 正常系と異常系の両方をテスト
- エラーハンドリングのテスト
- エッジケースの確認
- パフォーマンスが重要な部分にはベンチマークを検討

---

## モジュール設計

### 単一責任の原則

各クレート/モジュールは明確な単一の責任を持つように設計：

- **font**: フォント関連の処理のみ
- **parser**: テキストのパースのみ
- **pdf_gen**: PDF 生成のみ

### 依存関係の管理

- 循環依存を避ける
- 共通型は`types`クレートで定義
- 設定関連は`read_config_file`クレートに集約

---

## CLI コマンド

現在実装されているコマンド：

### `build`

テキストファイルから PDF を生成する主要な機能です。

```
cargo run build <text_file_path>
```

### `variation-axes`

指定されたフォントのバリアブルフォント軸情報を取得します。

```
cargo run variation-axes <font_path> [--font-index <index>]
```

### `ttc-names`

TrueType Collection（TTC）ファイル内のフォント名一覧を取得します。

```
cargo run ttc-names <ttc_file_path>
```

### `script-langs`

フォントでサポートされているスクリプトと言語情報を取得します。

```
cargo run script-langs <font_path> [--font-index <index>]
```

---

## 開発フロー

1. **設計**: 変更の影響範囲を確認し、適切なモジュールを選択
2. **実装**: 上記のコーディング規約に従って実装
3. **テスト**: ユニットテストを追加し、動作を確認
4. **ドキュメント**: ドキュメントコメントを更新
5. **フォーマット**: `cargo fmt`でコードをフォーマット
6. **リント**: `cargo clippy`で問題がないか確認
7. **ビルド**: `cargo build --release`でリリースビルドを確認

---

## よくある実装パターン

### フォント処理

- HarfRust によるテキストシェーピング
- Allsorts によるフォントのサブセット化
- バリアブルフォントの軸設定と検証

### PDF 生成

- `pdf-writer`クレートを使用
- CID フォントによる多言語対応
- ToUnicode CMap による文字抽出の実装

### 設定ファイル

- TOML フォーマット
- パスの正規化と検証
- バリデーションルールの厳格な適用

### ロギング

- `tracing`クレートを使用
- 重要な処理にログを追加してデバッグを容易に
