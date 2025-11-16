# テストフォント

このディレクトリには自動テストに使用するフォントファイルを配置します。

## 必要なファイル

1. NotoSansJP-Regular.ttf
   - ダウンロード元: [Google Fonts - Noto Sans JP](https://fonts.google.com/noto/specimen/Noto+Sans+JP)
   - バージョン: 最新版を使用
   - 用途: 基本的なフォント解析とシェーピングテスト

2. NotoSansJP-Variable.ttf (可変フォントテスト用)
   - ダウンロード元: [Google Fonts - Noto Sans JP](https://fonts.google.com/noto/specimen/Noto+Sans+JP)
   - バージョン: 最新版を使用
   - 用途: 可変フォント検出とエラー処理のテスト

## 設定手順

1. 上記のリンクから必要なフォントファイルをダウンロード
2. このディレクトリに配置
3. ファイル名が正確に一致していることを確認

## フォントライセンス

Noto Sans JP は SIL Open Font License 1.1 の下で提供されています。
詳細は [SIL Open Font License](https://scripts.sil.org/OFL) を参照してください。

## テスト実行

フォントファイルを配置後、以下のコマンドでテストを実行できます:

```bash
# 全てのテストを実行（フォントファイルが必要）
cargo test

# フォントファイルが必要なテストをスキップ
cargo test -- --skip-ignored
```
