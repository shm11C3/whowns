# whowns

[![CI](https://github.com/shm11C3/whowns/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/shm11C3/whowns/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/shm11C3/whowns?sort=semver)](https://github.com/shm11C3/whowns/releases/latest)
[![Platforms: macOS and Linux](https://img.shields.io/badge/platforms-macOS%20%7C%20Linux-blue)](#現在の境界)
[![License: MIT](https://img.shields.io/github/license/shm11C3/whowns)](LICENSE)

[English](README.md)

「この `node`、何で入れたんだっけ？」を個別に逆引きし、管理元と次に使うべき管理コマンドを証拠つきで説明する診断 CLI です。

`whowns` は `who owns` を6文字に圧縮した名前です。

一覧の作成自体ではなく、次の疑問に答えることを中心価値にしています。

- 今実行されるのはどの実体か
- PATH の後ろに別バージョンが隠れていないか
- ランタイムを管理するツールと、そのツール自体の導入元は何か
- その判断を支えるパス、シンボリックリンク、receipt、管理ツールの照会結果は何か
- 更新・削除するとき、どの管理ツールを使うべきか

Rust の単体バイナリなので、利用者側に Node.js やPythonなどの追加ランタイムは不要です。

## 目次

- [インストール](#インストール)
- [個別逆引き](#個別逆引き)
- [一覧表示](#一覧表示)
- [JSON](#json)
- [確信度](#確信度)
- [検出できる管理元](#検出できる管理元)
- [開発用ビルド](#開発用ビルド)
- [現在の境界](#現在の境界)

## インストール

[GitHub Releases](https://github.com/shm11C3/whowns/releases)を正式なバイナリ配布経路とします。推奨installerはOSとCPUを判定し、対応するarchiveをダウンロードして、Releaseのchecksum manifestと照合してから単体バイナリをインストールします。

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/shm11C3/whowns/releases/latest/download/install.sh | sh
```

デフォルトでは`$HOME/.local/bin/whowns`と、短縮aliasの`$HOME/.local/bin/wio`を作成します。installerのオプションは`sh -s --`の後ろへ渡します。

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/shm11C3/whowns/releases/latest/download/install.sh \
  | sh -s -- --no-alias

curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/shm11C3/whowns/releases/latest/download/install.sh \
  | sh -s -- --bin-dir /usr/local/bin
```

Release archiveを先にダウンロードして、内容を確認してからインストールする方法も利用できます。

```sh
tar -xzf whowns-v0.1.0-aarch64-apple-darwin.tar.gz
cd whowns-v0.1.0-aarch64-apple-darwin
./install.sh
```

checksum、artifact attestation、対応architecture、公開手順は [docs/RELEASING.md](docs/RELEASING.md) に記載しています。

ソースからCargoで導入することもできます。

```sh
cargo install --path . --locked

whowns node
```

## 個別逆引き

```console
$ whowns node
node
├── ● active
│   ├── executable: /usr/local/bin/node
│   ├── ownership: node → macOS Installer (.pkg) [confirmed]
│   └── actions (macOS Installer (.pkg))
│       ├── inspect: pkgutil --pkg-info org.nodejs.node.pkg
│       └── note: Update by installing a newer package from the same vendor. ...
└── ○ shadowed
    ├── executable: /opt/homebrew/bin/node
    ├── resolves to: /opt/homebrew/Cellar/node/25.6.1_1/bin/node
    ├── ownership: node → Homebrew [probable]
    └── actions (Homebrew)
        ├── inspect: brew info node
        ├── update: brew upgrade node
        └── remove: brew uninstall node
```

terminal treeによって、実体、所有関係、操作案内のつながりを視覚的に追えます。`active` は現在実行されるもの、`shadowed` はインストール済みだが PATH の優先順位で隠れているものです。更新・削除コマンドは案内するだけで、自動実行しません。

一方で、`whowns` は所有関係を確認するために他のプログラムを実行することがあります。既知のパッケージ・バージョン管理ツール（`mise which`、`pyenv which`、`pkgutil --file-info` など）を、読み取り専用のサブコマンドで呼び出します。この照会の安全策は[検出できる管理元](#検出できる管理元)を参照してください。

詳細な検出根拠と多段所有関係は `--explain` で表示します。

```sh
whowns node --explain
whowns rustc cargo --explain
```

たとえば環境から確認できる場合、次のような所有チェーンになります。

```text
node → nvm [probable] → Homebrew [probable]
rustc → rustup [confirmed] → rustup installer [probable]
```

導入元に到達するまで上流を解決します。循環、または8つを超える所有者を検出した場合は、安全のため探索を止め、停止理由を根拠に持つ `unconfirmed source` ノードを末尾に追加します。

## 一覧表示

一覧も個別診断と同じ `OwnershipGraph` から生成します。このグラフはコマンド単位のモデルであり、複数の PATH resolution を持てます。一方、各 resolution が持つ所有関係は、近い管理元から順に並ぶ1本の線形 chain です。専用の検出ロジックはありません。

```sh
whowns --all
whowns --all --explain
whowns --all --show-missing
```

一覧は共通グラフのactive実体、先頭の管理元、確信度、shadowed件数を要約します。`--explain` を付けると同じグラフの詳細を続けて表示します。

## JSON

個別表示と一覧表示のどちらも、同じversion付き機械可読documentを出力します。

```sh
whowns node --json
whowns --all --json
```

概念モデルは次の構造です。

```text
JSON document
├── schema_version: 1
└── graphs: OwnershipGraph[] (command とその PATH resolutions)
    └── Resolution[] (active / shadowed, path, real_path)
        └── OwnershipNode[] (近い管理元から順番に並ぶ)
            ├── id (安定 ID) / name (表示名)
            ├── kind
            ├── package / version
            ├── Confidence
            ├── Evidence[]
            └── ActionGuide
```

- `Resolution`: PATH 上の有効な実体と隠れている実体
- `OwnershipNode`: `runtime -> manager -> upstream manager -> installation source` と近い順に並ぶ線形 chain の1要素
- `id`: `homebrew`、`sdkman`、`macos_installer` のような安定した機械可読の所有者 ID。`name` は表示用テキストであり、変更しても `id` には影響しない
- `Evidence`: PATH、symlink、filesystem、`pkgutil`、パッケージ照会、管理ツール照会などの根拠
- `Confidence`: `confirmed`、`probable`、`unknown`
- `ActionGuide`: 確認・更新・削除の候補コマンドと注意事項

`schema_version` は `whowns` package versionとは独立してJSON contractをversion管理します。0.x系列では、optional fieldの追加はschema version 1内の互換変更とします。fieldの削除・rename、型や意味の変更、安定owner `id`の変更には新しいschema versionが必要です。consumerは対応するschema versionを選び、未知のfieldを無視してください。表示用の`name`はschema versionを変えずに変更される可能性があるため、機械的な識別には`id`を使用してください。

## 確信度

- `confirmed`: package databaseやreceiptが対象ファイルの所有を記録している、または管理ツール照会が解決済みの実体を返した
- `probable`: 既知の管理パス配置、対象ファイルに直接結び付かない導入済みreceipt、OS標準パスから管理元をかなり絞れるが、直接の所有記録はない
- `unknown`: 認識済みの管理元がなく、安全な更新・削除方法を決められない

確信度は各所有候補の型付き `Evidence` から導出します。detectorは観測した事実だけを返し、`confirmed` を直接指定できません。`--explain` では、確定根拠となったreceipt、package query、実体と一致したmanager query、またはそれより弱いpath evidenceを確認できます。

`/usr/local` にあるという理由だけで「手動インストール」とは断定しません。vendor installer、パッケージマネージャ、手動コピーのいずれもあり得るため、`unconfirmed owner` と未確認理由を返し、更新・削除コマンドは生成しません。

## 検出できる管理元

- Nix、Homebrew、MacPorts
- nvm、fnm、Volta、mise、asdf
- pyenv、rbenv、SDKMAN!、uv、rustup、`cargo install`
- Deno/Bun のインストーラ用ディレクトリ、pnpm home
- macOS Installer の package receipt（`pkgutil`）とpython.orgのFramework配置
- Linux の dpkg、RPM、pacman、apk
- OS標準パス

既知の管理ツールについては、`which` や `current` 相当の読み取り専用照会を実行し、その結果を `Evidence` に追加します。MacPorts prefix配下のパスには `port -q provides <path>` でローカルregistryを照会し、パスだけなら `probable`、registryが所有者を返した場合は `confirmed` とします。

これらの照会は、生のsubprocess呼び出しではなく、単一の実行ポリシーを経由します。

- 照会対象は `whowns` がすでに PATH 上で解決済みの実行ファイルです。裸のコマンド名を渡して改めて PATH 探索させることはしません。二つの探索の間に PATH が変わっても、`whowns` が検査したのと同じ実行ファイルに照会が向くようにするためです。
- 各照会は数秒以内に終わらなければ強制終了します。管理ツールがハングしたり応答が遅くても、単体の検査や `--all` 全体をブロックしません。
- 同一の照会は1回の `whowns` 実行につき一度しか実行しません。`--all` で複数のランタイムや解決結果にまたがって同じ照会が繰り返される場合は、キャッシュ結果を再利用します。
- 取得する出力サイズには上限があります。管理ツールが過剰な出力を返してもメモリを消費し尽くしません。
- タイムアウトや起動に失敗した照会は、常に標準エラー出力へ `note:` 行として記録します。所有者が判明した後に確認のため実行する管理ツール照会(`which`/`current` 相当)は、非ゼロ終了を含むその結果を、さらにその所有者の `Evidence` にも記録します。劣化した確認照会が結果を静かに変えてしまうことはありません。
- 照会は親プロセスの環境変数をそのまま引き継ぎます。管理ツールは `HOME` などの変数から自身のデータディレクトリを解決するため、環境をクリアしたり偽装したりすると、安全になるどころか誤った回答を招きます。

## 開発用ビルド

```sh
cargo test
cargo build --release
./target/release/whowns node
```

外部Rustクレートには依存していません。

## 現在の境界

macOS/Linux の PATH 上にある実行ファイルが対象です。OSや各パッケージマネージャに登録された全パッケージの棚卸し、Windows、シェル設定を遡ったPATH設定元の特定、アンインストールの自動実行は扱いません。

シェル設定を解析できない場合など、バージョン管理ツール自体の導入元が分からなければ、所有チェーンの末尾を `unconfirmed source [unknown]` として止めます。
