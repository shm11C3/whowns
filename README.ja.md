# whowns

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

- [個別逆引き](#個別逆引き)
- [一覧表示](#一覧表示)
- [JSON](#json)
- [確信度](#確信度)
- [検出できる管理元](#検出できる管理元)
- [インストール](#インストール)
- [開発用ビルド](#開発用ビルド)
- [現在の境界](#現在の境界)

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
    ├── ownership: node → Homebrew [confirmed]
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
node → nvm [confirmed] → Homebrew [confirmed]
rustc → rustup [confirmed] → rustup installer [probable]
```

## 一覧表示

一覧も個別診断と同じ `OwnershipGraph` から生成します。専用の検出ロジックはありません。

```sh
whowns --all
whowns --all --explain
whowns --all --show-missing
```

一覧は共通グラフのactive実体、先頭の管理元、確信度、shadowed件数を要約します。`--explain` を付けると同じグラフの詳細を続けて表示します。

## JSON

個別表示と一覧表示のどちらも同じ機械可読モデルを出力します。

```sh
whowns node --json
whowns --all --json
```

概念モデルは次の構造です。

```text
OwnershipGraph (command)
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
- `OwnershipNode`: `runtime -> version manager -> installation source` の順序付き所有関係
- `id`: `homebrew`、`sdkman`、`macos_installer` のような安定した機械可読の所有者 ID。`name` は表示用テキストであり、変更しても `id` には影響しない
- `Evidence`: PATH、symlink、filesystem、`pkgutil`、パッケージ照会、管理ツール照会などの根拠
- `Confidence`: `confirmed`、`probable`、`unknown`
- `ActionGuide`: 確認・更新・削除の候補コマンドと注意事項

## 確信度

- `confirmed`: package receipt、Nix store、Homebrew Cellar、既知のバージョン管理ディレクトリなど、強い所有証拠がある
- `probable`: パス規約とツール構造から管理元をかなり絞れるが、receiptのような直接の所有記録ではない
- `unknown`: 認識済みの管理元がなく、安全な更新・削除方法を決められない

`/usr/local` にあるという理由だけで「手動インストール」とは断定しません。vendor installer、パッケージマネージャ、手動コピーのいずれもあり得るため、`unconfirmed owner` と未確認理由を返し、更新・削除コマンドは生成しません。

## 検出できる管理元

- Nix、Homebrew、MacPorts
- nvm、fnm、Volta、mise、asdf
- pyenv、rbenv、SDKMAN!、uv、rustup、`cargo install`
- Deno/Bun のインストーラ用ディレクトリ、pnpm home
- macOS Installer の package receipt（`pkgutil`）とpython.orgのFramework配置
- Linux の dpkg、RPM、pacman、apk
- OS標準パス

既知の管理ツールについては、`which` や `current` 相当の読み取り専用照会を実行し、その結果を `Evidence` に追加します。

これらの照会は、生のsubprocess呼び出しではなく、単一の実行ポリシーを経由します。

- 照会対象は `whowns` がすでに PATH 上で解決済みの実行ファイルです。裸のコマンド名を渡して改めて PATH 探索させることはしません。二つの探索の間に PATH が変わっても、`whowns` が検査したのと同じ実行ファイルに照会が向くようにするためです。
- 各照会は数秒以内に終わらなければ強制終了します。管理ツールがハングしたり応答が遅くても、単体の検査や `--all` 全体をブロックしません。
- 同一の照会は1回の `whowns` 実行につき一度しか実行しません。`--all` で複数のランタイムや解決結果にまたがって同じ照会が繰り返される場合は、キャッシュ結果を再利用します。
- 取得する出力サイズには上限があります。管理ツールが過剰な出力を返してもメモリを消費し尽くしません。
- タイムアウトや失敗した照会は、対象の所有者が存在すればその `Evidence` に、存在しなければ標準エラー出力の `note:` 行として記録します。劣化した照会が結果を静かに変えてしまうことはありません。
- 照会は親プロセスの環境変数をそのまま引き継ぎます。管理ツールは `HOME` などの変数から自身のデータディレクトリを解決するため、環境をクリアしたり偽装したりすると、安全になるどころか誤った回答を招きます。

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
