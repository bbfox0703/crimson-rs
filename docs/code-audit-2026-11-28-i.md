# Code audit — I. 建置、CI、測試基礎設施、腳本與資料（2026-11-28）

依 [`code-audit-2026-11-28-plan.md`](code-audit-2026-11-28-plan.md) 的 I 區塊做靜態審計。**沒有修改任何程式碼**，沒有建置或執行 crimson-rs（沒有跑 `cargo build/test/clippy`、`maturin`、`cargo audit`），沒有派發 subagent，沒有碰 PAPGT trailer，也沒有讀寫遊戲行程。

- **基準：** `dev` @ `e465942`。下文行號都以這個 commit 的工作樹為準。
- **範圍：** `Cargo.toml`、`Cargo.lock`、`pyproject.toml`、`.cargo/config.toml`、`.github/workflows/ci.yml`、`.gitignore`、`src/lib.rs`（`#[cfg(test)] mod tests`，104–1460 以後）、`src/test_fixtures.rs`、`src/repo_doc_budget.rs`、`src/binary/gamedata_layout.rs`（test-only resolver，B 區塊的範圍，但它決定所有 live 測試怎麼 skip，所以這裡從「測試設施」角度再看一次）、`src/c_abi/character_info.rs:766-8127`、各表 `mod.rs` 與 `c_abi/*.rs` 的 live 測試 helper（抽樣）、`scripts/`（重點：`setup-fork-guards.ps1`、`dump_gamedata_keys.py`、`extract_gamedata_bin.py`、`update_ce_table.py`、`export_for_ce.py:340-512`、`README.md`、`CLAUDE.md`）、`data/`（`gamedata-keys-2.05.01`、`gamedata-keys-2.06` 逐檔計數）、`references/store_info.hexpat`、`tools/git-crypt.exe`（只看文件，不執行）、`../crimson-rs-private/saves/`（只列檔名）。
- **方法：** 逐行閱讀與 `Grep` 計數。測試數以 `#[test]` / `#[ignore` 出現次數計算（巨集展開的測試不在內）。

### 這次沒做到的事

1. **來源歸屬沒有用 git 驗證。** 本 session 的 `git log` / `git blame` 需要額外核准，沒有執行。「本次引入／既有問題」改以 [`dev-log.md`](dev-log.md)（#115–#127 與 `e465942` 的條目）和程式碼內的註解判斷，各條會註明依據。
2. **`G:\` 與 `X:\` 不在本 session 允許的目錄內**，無法確認版本封存的實際形狀；I-04 因此是 plausible。
3. **沒有建置 `--features c_abi` 的 release dll 或檢查其 import table**（`dumpbin /dependents`），計畫書審計重點 4 只能做依賴分析（見 I-09 與文末排除項）。
4. **沒有跑 `cargo audit` / `cargo deny`**（需要核准與網路；計畫書也規定只能在本機執行）。審計重點 7 只做了 `Cargo.lock` 的靜態盤點（I-21）。

### 確信度與來源歸屬的定義

- **confirmed**：只靠原始碼、設定檔或資料檔的文字就能確定的事實（「沒有這個檢查」、「CI 不帶這個 feature」、「兩份文件互相矛盾」、「檔案裡有 N 行」）。
- **plausible**：行為上的後果是追程式路徑推出來的，沒有用位元組或實際重現驗證。依 CLAUDE.md 與計畫書一律標 plausible。
- **本次引入**：2.06 移植週期內引入（dev-log #115–#127，2026-11-27，以及 `e465942`，2026-11-28）。
- **既有問題**：早於 #115。

---

## 摘要

| # | 標題 | 嚴重度 | 確信度 | 來源 |
|---|---|---|---|---|
| I-01 | CI 的 `cargo test --lib` 不帶 `c_abi`：約 397 個可執行測試中有約 286 個（所有 `src/c_abi/` 與 27 張表模組的測試，含純合成測試）在 CI 從不執行；CLAUDE.md 的「Test」指令也是同一個子集 | medium | confirmed | 既有問題 |
| I-02 | live 測試把「install 在、但讀不出來」當成「沒有 install」：`gamedata_layout::extract_bin` / `extract_dlc_bin` / `paloc_*` 與 27 個表模組的 `find_table_bytes` 對任何錯誤都回 `None` → 印 skip → 綠燈；所有 DLC merge 測試都走這條 | medium | confirmed（程式碼）／plausible（漂移時的後果） | 既有問題（DLC 部分本次引入） |
| I-03 | `src/lib.rs` 的測試混用寫死的 `GAME_DIR` 與 `CRIMSON_GAME_ROOT`：設了覆寫時，skill 檔名依覆寫 install 的佈局解析、卻從 `D:\` 抽檔；6 個測試完全不看覆寫 | low | confirmed | 既有問題 |
| I-04 | `test_skill_roundtrip_cross_version` 要嘛永遠 skip，要嘛在 `expect("dir not found")` panic：路徑形狀與 scripts/CLAUDE.md 記載的封存不符，佈局又取自 live install | low | confirmed（程式碼）／plausible（G: 的實際形狀） | 既有問題 |
| I-05 | 存檔 encoder 的兩個 roundtrip 測試只讀 `%LOCALAPPDATA%` 的 live 存檔，不讀 `crimson-rs-private` fixtures；9 個 fixture 中有 3 個沒有任何測試引用 | low | confirmed | 既有問題（fixture 搬家是本次） |
| I-06 | live 存檔測試釘死 `slot107`（20+ 處複製貼上），驗的是那個 slot 碰巧是哪一版，不一定是最新版 | low | confirmed（程式碼）／plausible（後果） | 既有問題 |
| I-07 | 過時的版本註解與標籤：`lib.rs:117`「parser targets 1.05」、`:151`「1.04 baseline」、`test_skill_roundtrip_1_05`、`:560`「1.12 save」、`:790`「live 1.06 save」；scripts/CLAUDE.md:72 說 roundtrip 測試在版本不符時會 skip（實際會失敗） | nit | confirmed | 既有問題 |
| I-08 | `test_full_roundtrip` 讀 `out\iteminfo.pabgb`：內容是最後一次 `export_for_ce.py` 的輸出，與 live install 無綁定，只測 base、不測 `dlc_01`；路徑以 `\` 拼接，非 Windows 永遠 skip | low | confirmed | 既有問題 |
| I-09 | CI 從不建置真正出貨的產物：只跑 Ubuntu，不建 `--features c_abi` 單獨組合、不建 Windows MSVC、不經過 `.cargo/config.toml`、不檢查 dll 的 import table | low | confirmed（CI 範圍）／plausible（零 DLL 依賴） | 既有問題 |
| I-10 | `.cargo/config.toml` 的註解過時：引用已刪除的 `build.yml` 與 windows-gnu wheel；現在 `maturin develop` 在 MSVC 上建的 `.pyd` 也被 `+crt-static`，與註解的理由相反；只覆蓋 x86_64 | low | confirmed | 既有問題 |
| I-11 | CI 環境沒有釘選：CLAUDE.md 說「CI pins 3.12」但 workflow 沒有 setup-python；Rust toolchain 浮動於 `stable`；actions 以 tag 釘選；沒有 `permissions:` | nit | confirmed | 既有問題 |
| I-12 | 模組層級的 `allow(dead_code)` 讓「全 feature 嚴格 clippy 抓得到真正的死碼」（`lib.rs:1-11`）不成立；`encoder.rs` 的允許理由已過時 | low | confirmed | 既有問題 |
| I-13 | `claude_md_files_stay_under_40_kb` 量的是工作樹位元組：Windows（CRLF）與 CI（LF）的上限差一個行數；也會掃到未追蹤目錄 | nit | confirmed | 本次引入 |
| I-14 | `dump_gamedata_keys.py`：`--version` 預設 `1.08`，照 runbook 不帶參數跑會覆寫已提交的 `data/gamedata-keys-1.08/`；表缺失或形狀不明時仍 exit 0；吞掉所有例外；docstring 與程式不符 | low | confirmed | 既有問題（2.06 形狀偵測本次） |
| I-15 | key snapshot 不涵蓋 bridge 實際讀的 `faction`、`stringinfo`，也不涵蓋任何 `*.dlc_01` 表 | low | confirmed | 既有問題／本次（`faction`、DLC 於 #116/#119 起被 bridge 使用） |
| I-16 | `extract_gamedata_bin.py` 沒有覆寫保護：版本封存是不可重建的 ground truth，但 `--version` 標錯就直接覆蓋；子目錄被攤平且不檢查同名衝突；bin 檔沒有雜湊 manifest | medium | confirmed（程式碼）／plausible（情境） | 既有問題 |
| I-17 | `update_ce_table.py` 的 `props_from_bin` 少了空 `inventory_info_list` 的防護；docstring 範例 `--props-bin …/2.05` 在只讀 2.06 的 parser 下讀不了；`read_paver` 不看覆寫 | low | confirmed（程式碼）／plausible（IndexError 是否命中） | 既有問題 |
| I-18 | 腳本各自維護 install 候選清單、不讀 `CRIMSON_GAME_ROOT`；多個腳本寫死個人路徑；`scripts/README.md` 的索引缺 step-0 工具與 fork guard | nit | confirmed | 既有問題 |
| I-19 | `setup-fork-guards.ps1`：原生指令失敗不會停（`$ErrorActionPreference` 管不到）；hook 只擋 `git push`，擋不到 `gh` API 操作與上游改名後的 URL；既有 hook 存在時只發警告 | low | confirmed（程式碼）／plausible（PowerShell 版本行為、改名情境） | 既有問題 |
| I-20 | `tools/git-crypt.exe` 的來源、版本、雜湊與授權沒有任何文件 | nit | confirmed | 既有問題 |
| I-21 | 依賴：沒有 `cargo audit` / `deny.toml` / CI 稽核步驟；`Cargo.lock` 有兩代 RustCrypto 並存 | nit | confirmed（缺流程）／未驗證（advisory） | 既有問題 |
| I-22 | `character_info.rs` 的測試其實是 37 個與 characterinfo 無關的調查 probe（計畫書寫 49 個測試函式，實際 44 個 `#[test]`）；probe 之間有檔案相依；live helper 被複製約 60 份 | low | confirmed | 既有問題 |
| I-23 | `references/store_info.hexpat` 沒有版本標記，描述的是 2.04 之前的列頭，`BString` 定義與專案慣例相反 | nit | confirmed | 既有問題 |

沒有 critical 或 high。I 區塊的問題集中在一件事：**綠燈的意義比看起來小。** CI 只跑約四分之一的測試（I-01），這四分之一裡 `src/lib.rs` 的 21 個非 ignored 測試在 CI 全部是空跑（都依賴 live install、live 存檔或 fixtures）；到了有 install 的機器上，一類 live 測試又會把「讀不出來」當成「沒有 install」（I-02），所以版本漂移或 DLC 佈局改變時，部分覆蓋會靜默消失，而不是失敗。另外有兩個不可重建的資料產物（版本封存 I-16、已提交的 key snapshot I-14）只靠使用者手動不打錯參數來保護。

---

## Findings

### I-01 CI 不帶 `c_abi` 跑測試：約 72% 的測試從不在 CI 執行

- **位置：** `.github/workflows/ci.yml:57-58`（`cargo test --lib`）；對照 `ci.yml:48-51`（`cargo check --lib`、`cargo clippy --all-targets --lib --features c_abi,python`）；`CLAUDE.md:31`（Development 的「**Test:** `cargo test --lib`」）與 `CLAUDE.md:37`（Active workstreams 的 `cargo test --lib --features c_abi -- --test-threads=4`）。
- **問題與根因：** `src/lib.rs:18-93` 把 `c_abi` 模組與 27 個表模組（`character_info`、`craft_tool_info`…`trigger_region_info`）都放在 `#[cfg(feature = "c_abi")]` 之下，它們的 in-file 測試也跟著只在帶 `c_abi` 時編譯。CI 的 test 步驟沒帶 feature，所以這些測試從不執行；clippy 步驟雖然帶了 `c_abi,python` 並用 `--all-targets` 把它們編譯一遍，但不執行。
  - 計數（`#[test]` / `#[ignore` 出現次數）：全 repo 447 / 50 → 可執行約 397（dev-log #125 記 398 passed，差的 1 個應是巨集產生的測試）。default build 編得到的模組（`binary`、`crypto`、`item_info`、`skill_info`、`string_info`、`save`、`lib.rs`、`repo_doc_budget`）共 114 / 3 → **CI 實際執行約 111 個**；其餘約 **286 個只在本機帶 `c_abi` 時才跑**。
  - 不跑的不只是 live 測試，還有完全不需要 install 的合成測試：例如 `c_abi_characterinfo_null_args`（`character_info.rs:1146`）、`c_abi_characterinfo_empty_bytes_yields_empty_handle`（`:1210`）、`src/c_abi/paver.rs` 的 12 個測試（`PARSER_TARGET_*` 版本閘門，CLAUDE.md 的不變式之一）、`src/c_abi/mod.rs` 的 44 個存檔編輯測試中不依賴 fixtures 的部分、`element_template.rs` 的 CRET 格式測試。
  - CLAUDE.md 自相矛盾：`:31` 的「Test」指令是 CI 那個子集，`:37` 的才是完整套件。照 `:31` 跑的人會以為跑完了。
- **重現情境：** 在 `src/c_abi/paver.rs` 把版本比較改壞（例如只比 major），推上 PR：clippy 通過、`cargo test --lib` 通過，`main` 的必要檢查綠燈。
- **嚴重度：** medium。C ABI 是下游（CrimsonAtomtic）唯一使用的介面，`main` 的分支保護只看這個 check。
- **確信度：** confirmed（CI 設定與 cfg 閘門都是文字事實；計數方式如上）。
- **來源：** 既有問題（CI 的 test 步驟早於 2.06；dev-log #115–#127 每條都記錄本機帶 fixtures 的測試數，而不是 CI 的）。

### I-02 live 測試把「install 在、但讀不出來」當成「沒有 install」

- **位置：**
  - `src/binary/gamedata_layout.rs:56-71`：`resolved()` 在 PAMT 讀不到、**parse 失敗**、或**沒有任何已知佈局的目錄**時都回 `0`（最新佈局），不區分「沒有 install」與「install 的佈局沒見過」。
  - `src/binary/gamedata_layout.rs:102-110`（`extract_bin_from`）、`:134-155`（`paloc_files`）、`:164-180`（`paloc_blobs`）、`:189-205`（`paloc_bytes`）：PAMT parse、找目錄、找檔名、`paz::extract_file` 的任何失敗都 `.ok()?` → `None`。
  - 呼叫端把 `None` 一律當成 skip：例如 `src/c_abi/character_info.rs:8085-8095`（`c_abi_characterinfo_dlc_merge_live`）、`src/c_abi/faction_info.rs:567-578`，印的是「no game install」/「dlc_01 not installed」。
  - 表模組的 `find_table_bytes`（以 `src/craft_tool_info/mod.rs:70-109` 為代表，逐行讀過；其餘 `src/<table>_info/mod.rs` 的 `*_lossy_live` 測試共 26 個模組各有一份同形 helper，抽樣看過數個）：只有「`0008/0.pamt` 不存在」是真正的「沒有 install」，之後的 `PackMeta::parse(..).ok()?`、`find(..)?`、`extract_file(..).ok()?` 全部變成 skip。
- **問題與根因：** 「沒有 install」與「install 存在但 pipeline 壞了」走同一個回傳值。repo 裡其實已經有正確的寫法：`src/c_abi/live_titles.rs:26-33`（install 在就 `expect`，「so a broken pipeline cannot pass as a skip」）、`src/binary/paz.rs:1208-1221`、`src/c_abi/craft_tool_info.rs:82-96`（存在檢查之後一律 `assert`）。問題是這個慣例沒有套到共用 helper 上。
- **重現情境：**
  1. **容器搬家（2.01 的情況再發生一次）：** `0008/0.pamt` 存在，但表搬到新目錄。`resolved()` 回 0，`find(|d| d.path == bin_dir())` 找不到 → 26 個 `*_lossy_live` 測試全部印「no game install」並通過。對應的 `c_abi_*_live` 測試會失敗（它們用 assert 版的 `extract_file`），所以漂移不會完全沒被發現，但核心解析器那一側的覆蓋靜默消失。
  2. **DLC 佈局改變（2.04 的教訓：「Check a new layout against every table that shares it, DLC included」）：** `dlc/dlc_01/0008/0.pamt` 在，但 `faction.dlc_01` 改名、或 DLC 的 PAMT 用了新形狀 → `extract_dlc_bin` 回 `None` → 所有 DLC merge 測試（`character_info.rs:8084`、`faction_info.rs:567`、`faction_node_info.rs:695`、`item_part_prefab.rs:2280` 等）印「dlc_01 not installed」並通過。DLC merge 沒有 assert 版的對照測試。
  3. **PAZ 解壓縮 regression：** `extract_file` 回錯 → 表模組測試 skip。
- **嚴重度：** medium。這直接違反計畫書跨區塊關注點「版本漂移要明確失敗」，而且發生的時機正是最需要測試的時候（新 patch）。
- **確信度：** confirmed（回傳路徑是文字事實）／plausible（「新 patch 時會靜默通過」是推論，沒有用改名的 PAMT 實測）。
- **來源：** 既有問題（`find_table_bytes` 形狀早於 2.06；`gamedata_layout` 的 fallback 來自 2.01）；`extract_dlc_bin` 與 DLC merge 測試是本次引入（dev-log #119、#124）。

### I-03 `src/lib.rs` 混用寫死的 `GAME_DIR` 與 `CRIMSON_GAME_ROOT`

- **位置：** `src/lib.rs:116`（`GAME_DIR`）、`:128-135`（`PAPGT_PATH`、`PAMT_PATH` 也寫死）、`:170-232`（`test_papgt_parse/roundtrip`、`test_pamt_parse/roundtrip`）、`:327-335`、`:347-356`（skill 兩個測試傳 `GAME_DIR`）、`:419-452`（`extract_skill_from_archive`）、`:460-521`（`test_game_dir_papgt_pamt_checksums`）；對照 `:241`、`:284`（paloc 測試走 `gamedata_layout::game_root()`，會讀覆寫）。另有 `src/c_abi/character_info.rs:3014`（ignored probe，寫死且不讀覆寫）。
- **問題與根因：** 計畫書待查事項 (I)「`lib.rs:116` 的 `GAME_DIR` 是寫死的路徑」——**確認**：`lib.rs` 的 24 個測試裡沒有一處讀 `CRIMSON_GAME_ROOT`（`Grep` 計數：`lib.rs` 有 3 處 `SteamLibrary`、0 處 `CRIMSON_GAME_ROOT`）。更隱蔽的是 `extract_skill_from_archive(GAME_DIR, &gamedata_layout::header("skill"))`：**檔名與目錄**由 `gamedata_layout::resolved()` 依 `CRIMSON_GAME_ROOT` 的 install 決定（`:436`、`:441`），**檔案**卻從 `GAME_DIR` 抽。
- **重現情境：** `CRIMSON_GAME_ROOT` 指向保留的 pre-2.01 install（CLAUDE.md 說覆寫就是為了「a kept previous version」），`D:\` 是 2.06：skill 測試在 2.06 的 PAMT 裡找 `gamedata/binary__/client/bin` → `expect("dir not found")` panic（`:437`）。反過來，只有覆寫、沒有 `D:\` 的機器上，PAPGT/PAMT/skill/checksum 6 個測試靜默 skip。
- **嚴重度：** low。
- **確信度：** confirmed。
- **來源：** 既有問題（`CRIMSON_GAME_ROOT` 引入時沒有回頭改 `lib.rs`）。

### I-04 `test_skill_roundtrip_cross_version` 不是 skip 就是 panic

- **位置：** `src/lib.rs:378-392`、`:419-452`。
- **問題與根因：**
  1. 路徑是 `G:\我的雲端硬碟\temp\Crimson Desert\<ver>\0008\0.pamt`（`:380-383`、`:423-424`）。依 `scripts/CLAUDE.md` 的 Machine notes，G: 上的版本封存是 `…\Crimson Desert\bin_backup\<ver>\`，**扁平的 bin 檔、沒有 `0008/0.pamt`**；X: 那台則沒有 G:。照文件，這個測試在兩台機器上都只會走 `:425-430` 的 skip。
  2. 若 G: 上確實有完整的 1.03.01–1.05.01 install 樹，`extract_skill_from_archive` 會用 **live install** 解析出的 `bin_dir()`（2.06 的 `gamedata/binarystaticinfo__/bin`）去找 1.0x 的 PAMT，然後在 `:437` 的 `expect("dir not found")` panic，因為 1.0x 用的是 `binary__/client/bin`。
  3. CLAUDE.md 說 skill parser「target the latest patch only」，所以即使找到檔案，這個測試的前提（同一個 parser 讀 1.03/1.04/1.05）也已不成立。
- **重現情境：** 在 G: 那台、`CRIMSON_GAME_ROOT` 未設、`D:\` 為 2.06，`G:\…\Crimson Desert\1.03.01\0008\0.pamt` 存在 → panic；不存在 → 三個版本都 skip，測試通過。
- **嚴重度：** low（死測試，或必定紅燈的測試）。
- **確信度：** confirmed（程式路徑）／plausible（G: 的實際目錄形狀：G: 不在本 session 允許的目錄，未能查看）。
- **來源：** 既有問題。

### I-05 存檔 encoder 的 roundtrip 測試不讀 fixtures；3 個 fixture 沒人用

- **位置：** `src/lib.rs:1074-1080`（`test_save_body_block_roundtrip_per_block`）、`:1228-1234`（`test_save_body_full_roundtrip`）都只用 `collect_all_saves()`（`:987-1021`，`%LOCALAPPDATA%`）；`test_save_parse`、`test_save_write_parse_roundtrip`、`test_save_body_parse`、`test_save_body_decode_all_blocks`（`:597-804`）只用 `try_find_save()`（`:530-595`）。只有 `test_save_body_absent_markers_every_live_save`（`:846-857`）把 fixtures 加進來。
- **問題與根因：** dev-log #121 把 fixtures 搬到 `crimson-rs-private`，並說「the tests use as fixtures」，但「encoder 是 decoder 的確定性反函數」這個最重要的性質（per-block 大小、全 body 冪等）只在有 live 存檔的機器上驗證。計畫書 F 區塊說「這塊的審計必須在有 fixtures 的機器上跑」——有 fixtures 不夠，還要有 live 存檔。
  - `../crimson-rs-private/saves/` 共 9 個 `save.save`；`src/` 裡引用到的只有 6 個（`1.09`、`1.10`、`broken_save_after_length_change/` 三個、`sealed-artifact-challenge/engine-natural/before`）。`sealed-artifact-challenge/editor-edit/{before,after}` 與 `engine-natural/after` 沒有任何測試讀（`Grep "editor-edit|engine-natural/after"` 於 `src/` 無結果）。
- **重現情境：** 一台只 clone 了 `crimson-rs-private`、沒裝遊戲的機器（或 CI）：兩個 encoder roundtrip 測試印「no saves found」並通過。
- **嚴重度：** low（encoder 本身的 relocation 由 `:1300-1433` 兩個 fixture 測試覆蓋一部分；F 區塊的 relocation 議題不在此重複）。
- **確信度：** confirmed。
- **來源：** 既有問題（測試早於 fixture 搬家；搬家是 #121 本次引入，但沒有改變這兩個測試的資料來源）。

### I-06 live 存檔測試釘死 `slot107`

- **位置：** `src/lib.rs:560-567`（優先 `slot107`，註解寫「the current Crimson Desert 1.12 save」）；`src/c_abi/character_info.rs` 有 18 份 `entry.path().join("slot107")`（例如 `:1306`、`:3003`、`:3106`、`:7954`）；`src/c_abi/all_items.rs:862`、`:1289` 與 `src/c_abi/positions.rs:708` 的 `live_slot107_*`。
- **問題與根因：** `scripts/CLAUDE.md` 的 Machine notes 明說「Which slot holds which patch's save differs per machine, so check the slot's mtime instead of assuming」。測試卻以 slot 編號挑檔，`try_find_save` 也只回傳第一個找到的，不看 mtime 或存檔 header 的版本。
- **重現情境：** 某台機器的 `slot107` 是 1.12 時代的舊存檔、2.06 存檔在 `slot102`：`test_save_body_decode_all_blocks` 等驗的是 1.12 格式，2.06 存檔格式的 regression 不會被這幾個測試抓到（`test_save_body_absent_markers_every_live_save` 會掃全部 slot，不受影響）。
- **嚴重度：** low。
- **確信度：** confirmed（程式碼）／plausible（實際上哪台機器的 slot107 是哪一版，未查）。
- **來源：** 既有問題。

### I-07 過時的版本註解、標籤與文件敘述

- **位置與內容：**
  - `src/lib.rs:117-120`：「The parser targets Crimson Desert 1.05. The roundtrip test reads the current 1.05 binary」。**確認計畫書待查事項 (I) `lib.rs:117`**：與 CLAUDE.md 的「latest patch only — currently 2.06」不符。
  - `src/lib.rs:151`：skip 標籤「1.04 baseline binary」。
  - `src/lib.rs:327`、`:334`：`test_skill_roundtrip_1_05` / 「1.05 (live game install)」，實際讀的是 live（2.06）。
  - `src/lib.rs:560`：「slot107 — the current Crimson Desert 1.12 save」；`:790`：「against the live 1.06 save」。
  - `scripts/CLAUDE.md:72`：「The iteminfo / skill roundtrip tests are pinned to the local game install and skip cleanly when the install isn't on the expected version」。實際上 `lib.rs:432-442` 在 install 存在時一律 `unwrap` / `expect`，版本不符會失敗，不會 skip（失敗才是對的，錯的是文件）；反過來，真正會「skip cleanly」的是 I-02 那一類。
  - `src/save/body/encoder.rs:3-7`、`src/save/body/mod.rs:113-115`、`src/save/body/object.rs:95-98`：「until Phase B.2 wires a C ABI consumer」，B.2 早已出貨（見 I-12）。
- **重現情境：** 新 session 依 `lib.rs:117` 或 `scripts/CLAUDE.md:72` 判斷「紅燈是因為版本不符、可以忽略」。
- **嚴重度：** nit。
- **確信度：** confirmed。
- **來源：** 既有問題。

### I-08 `test_full_roundtrip` 讀的是「最後一次 export」，不是 live install

- **位置：** `src/lib.rs:121-124`、`:150-167`；寫入端 `scripts/export_for_ce.py:460`。
- **問題與根因：**
  1. `out\iteminfo.pabgb` 是 `export_for_ce.py` 每次執行時覆寫的檔案（`export_for_ce.py:460`），scripts/CLAUDE.md 的 runbook 還要求在新 patch 前把它複製到 `out\export-<old>\`。測試不檢查它屬於哪一版（沒有雜湊、沒有 paver），也不和 live install 比對。換 patch 後、重新 export 前，這個測試會拿新 parser 讀舊 body。
  2. 只測 base；`out/iteminfo.dlc_01.pabgb` 就在旁邊（`export_for_ce.py:471-472` 寫出）卻沒測。CLAUDE.md 的「byte-perfect on 7,068 base + 8,218 `dlc_01` items」在這個測試裡只有一半。（`c_abi_iteminfo_live_roundtrip` 在 `c_abi` 側另有覆蓋，屬 C 區塊。）
  3. 路徑用 `concat!(env!("CARGO_MANIFEST_DIR"), r"\out\iteminfo.pabgb")`，在 Linux 上是一個含反斜線的單一檔名，永遠不存在。CI 不受影響（本來就沒有檔案），但寫法不可攜。
- **重現情境：** 2.07 上線、parser 已更新、還沒跑 export → `test_full_roundtrip` 用 2.07 parser 讀 2.06 body，失敗（或碰巧通過）。
- **嚴重度：** low。
- **確信度：** confirmed。
- **來源：** 既有問題。

### I-09 CI 從不建置真正出貨的產物

- **位置：** `.github/workflows/ci.yml:27`（`runs-on: ubuntu-latest`）、`:48-58`；`Cargo.toml:17`（`crate-type = ["cdylib", "staticlib"]`）；`.cargo/config.toml:17-18`。
- **問題與根因：** 下游拿的是 `cargo build --release --features c_abi` 在 `x86_64-pc-windows-msvc` 上的 `crimson_rs.dll` / `crimson_rs.lib`。CI 編的組合只有 default（check）與 `c_abi,python`（clippy、debug、Linux）。因此：
  - `--features c_abi` **單獨**這個組合（真正出貨的）從不被編譯；`python` 單獨也不（H-18 已列）。四種組合裡 CI 只覆蓋兩種。
  - `.cargo/config.toml` 只對 MSVC target 生效，CI 從不經過它。
  - 計畫書審計重點 4「零 transitive DLL 依賴」沒有任何自動化守門（沒有 Windows job、沒有 `dumpbin /dependents` 或等價檢查）。
- **零 DLL 依賴的靜態判斷：** `Cargo.lock` 的執行期依賴（`chacha20`、`cipher`、`lz4_flex`+`twox-hash`、`flate2`+`miniz_oxide`、`hmac`、`sha2`、`cpufeatures`）都是純 Rust，沒有 `cc` / `-sys` 的原生依賴（`windows-sys`、`libc` 只用於系統呼叫）；`+crt-static` 讓 UCRT/VCRUNTIME 靜態連結。所以「除系統 DLL 外零依賴」的說法 **plausible**，但沒有建置驗證。
- **重現情境：** 某個只在 `c_abi` 用到的項目被誤放到 `#[cfg(feature = "python")]` 之下：`c_abi,python` 編得過、default 編得過（該項目在 default 下不需要），`--features c_abi` 單獨編不過，CI 綠燈。
- **嚴重度：** low（維護者本機會建 dll；風險是時間差）。
- **確信度：** confirmed（CI 範圍）／plausible（零 DLL 依賴）。
- **來源：** 既有問題（2026-08-09 移除 `build.yml` 後，Windows 建置就完全離開 CI）。

### I-10 `.cargo/config.toml` 的註解過時，`.pyd` 也被靜態連結 CRT

- **位置：** `.cargo/config.toml:12-16`、`:17-18`。
- **問題與根因：** 註解說「The Python wheel CI in `.github/workflows/build.yml` uses `x86_64-pc-windows-gnu` and is unaffected … static-linking there would just bloat the wheel」。`build.yml` 已在 2026-08-09 刪除（`ci.yml:6-9` 自己說的），現在 wheel 都是本機 `maturin develop --release` 在 MSVC 上建，`[target.x86_64-pc-windows-msvc] rustflags` 一樣套用 → `.pyd` 也是 `+crt-static`，正好是註解說「沒有好處」的情況。功能上 PyO3 擴充模組不跨邊界傳 CRT 物件，所以應該無害，但註解的前提已不存在。另外 `aarch64-pc-windows-msvc` 不在設定內，該 target 的 dll 會動態連結 CRT，與文件「zero transitive DLL deps」的承諾不一致（目前沒有人建 ARM64，只記錄）。
- **重現情境：** 讀註解的人以為 `.pyd` 不受影響。
- **嚴重度：** low。
- **確信度：** confirmed（設定與註解文字）。
- **來源：** 既有問題。

### I-11 CI 環境沒有釘選

- **位置：** `CLAUDE.md:32`（「CI is unaffected (it pins 3.12)」）；`.github/workflows/ci.yml:29-37`；`Cargo.toml`（無 `rust-version`）；repo 根目錄沒有 `rust-toolchain.toml`。
- **問題與根因：**
  1. workflow 沒有 `actions/setup-python`，PyO3 的 `abi3-py312` 用的是 runner image 內建的 `python3`。`ubuntu-latest` 目前內建 3.12，所以能過，但這不是「pin」。若 runner image 換成內建 <3.12 的版本（目前不太可能）或移除 `python3`，clippy 步驟會以 CLAUDE.md:32 描述的錯誤失敗。
  2. `dtolnay/rust-toolchain@stable` 讓 toolchain 浮動；搭配 `-D warnings`，新版 clippy 加的 lint 可以在沒有任何程式變更的情況下讓 `main` 的必要檢查轉紅（edition 2024 也需要 ≥1.85，但沒有宣告 MSRV）。
  3. 第三方 actions 以 tag（`@v6`、`@stable`、`@v2`）而非 commit SHA 釘選；workflow 沒有 `permissions:`，`GITHUB_TOKEN` 權限取決於 repo 預設。
  4. `push: [main, dev]` 加上 `pull_request:` 會讓 `dev → main` 的 PR 對同一個 commit 跑兩次（concurrency group 以 ref 區分，不會互相取消）。
- **重現情境：** GitHub 更新 stable Rust 後的第一個 PR 因新 lint 失敗。
- **嚴重度：** nit。
- **確信度：** confirmed。
- **來源：** 既有問題。

### I-12 模組層級的 `allow(dead_code)` 掩蓋了全 feature 建置的死碼

- **位置：** `src/lib.rs:1-15`（宣稱「the strict clippy gate … still fails on genuinely dead code」）；`src/item_info/mod.rs:1-6`（`item`、`keys`、`structs` 三個整個模組 `#[allow(dead_code)]`）；`src/save/body/encoder.rs:3-7`（`#![allow(dead_code)]`，理由「no in-crate caller reaches into these functions yet (B.2 adds the C ABI wrappers)」）；`src/string_info/mod.rs:48-52`、`src/string_info/pabgh.rs:21`（檔案層級）；項目層級另有 `src/binary/pamt.rs:12`、`papgt.rs:3,16,122`、`trie.rs:138`、`paloc.rs:211`、`save/body/decoder.rs:1305`、`save/body/mod.rs:115`、`save/body/object.rs:98`。
- **問題與根因：** `lib.rs` 的 `cfg_attr` 設計是「部分 feature 才放寬、全 feature 嚴格」，但上述模組自己在全 feature 下也放寬，所以 `c_abi,python` 的 clippy 抓不到這些模組裡的死碼。`encoder.rs` 的理由已過時：`encode_top_level_block` 現在由 `c_abi` 使用（`src/lib.rs:1076`、`:1301` 的測試也用它），B.2 已出貨。計畫書跨區塊關注點「`cfg_attr(allow(dead_code))` 不能掩蓋真正的 dead code」——這裡被掩蓋的是 ItemInfo、存檔 encoder、StringInfo 三個核心模組。
- **重現情境：** 在 `src/item_info/structs.rs` 留一個不再被使用的 helper：全 feature 嚴格 clippy 不報。
- **嚴重度：** low。
- **確信度：** confirmed。
- **來源：** 既有問題。

### I-13 CLAUDE.md 大小守門量的是工作樹位元組

- **位置：** `src/repo_doc_budget.rs:10`、`:14`、`:16-34`、`:47-53`。
- **問題與根因：**
  1. `std::fs::metadata(p).len()` 量的是工作樹檔案。CLAUDE.md 規定 `core.autocrlf=true`，Windows 工作樹是 CRLF；CI（Linux checkout）是 LF。實測工作樹：`CLAUDE.md` 18,258 B / 82 個 CR、`scripts/CLAUDE.md` 30,223 B / 177 個 CR、`data/CLAUDE.md` 13,326 B / 65 個 CR、`scripts/archive/CLAUDE.md` 2,269 B / 20 個 CR。所以同一份內容在本機的「大小」比 CI 多一個行數；一份 LF 時 40,900 B、200 行的檔案在 CI 通過、在本機失敗。CLAUDE.md 寫「40,960 bytes」沒有說是哪一種。
  2. `collect` 走整個工作樹，只跳過 `SKIP_DIRS` 與 `.crimson_rs_*`，不限於已追蹤檔案：被 `.gitignore` 的 `tmp/`、未追蹤的巢狀 clone 裡的 `CLAUDE.md` 也會被算進去。
  3. symlink 會被跳過（`file_type()` 不是 dir 也不是 file），這是對的，只記錄。
- **重現情境：** 把 `scripts/CLAUDE.md` 長到 LF 40,800 B（CRLF 約 41,000 B）：CI 綠、本機 `cargo test --lib` 紅。
- **嚴重度：** nit（方向是「本機比 CI 嚴」，不會讓超標的檔案溜過本機）。
- **確信度：** confirmed（程式與 CR 計數）。
- **來源：** 本次引入（`e465942`，dev-log 2026-11-28）。

### I-14 `dump_gamedata_keys.py`：預設會覆寫 1.08 snapshot，失敗也 exit 0

- **位置：** `scripts/dump_gamedata_keys.py:182`（`--version` 預設 `"1.08"`）、`:188-191`（輸出目錄預設 `data/gamedata-keys-<version>`、`mkdir(exist_ok=True)`）、`:159-163`（`write_keys` 直接覆寫）、`:166-170`（`extract_safe` 吞掉所有例外）、`:199-239`（missing / shape unknown 只列在 summary）、`:5-8` 與 `:80-88`（docstring 說 questinfo/stageinfo 走 PABGB anchor scan，程式實際走 PABGH）、`:127-139`（2.06 形狀只試 key 寬 4/2/1）。
- **問題與根因：**
  1. scripts/CLAUDE.md 的新 patch runbook 第 7 步只說「Run `dump_gamedata_keys.py` to write the new `data\gamedata-keys-<ver>\`」，沒有寫參數。不帶 `--version` 執行會把**目前版本**的 key 寫進已提交的 `data/gamedata-keys-1.08/`，覆寫 1.08 的歷史 snapshot。git 能救回，但在 diff 很大的新 patch PR 裡容易被一起提交。
  2. 任何表 `<pabgh missing>` 或 `<pabgh shape unknown>` 時腳本仍正常結束（exit 0），也不刪除目錄裡同名的舊檔；搭配「預設目錄已存在」，可能留下新舊版本混雜的 snapshot 目錄。
  3. `extract_safe` 把 stale `.pyd`（CLAUDE.md 的「check this FIRST」陷阱）造成的例外也吞成「missing」。
  4. 2.06 形狀不支援 8/12 byte key（計畫書 B 區塊列的 key 寬度）；目前 30 張表沒有用到，只記錄。
- **重現情境：** 2.07 上線後在 repo 根目錄執行 `python scripts\dump_gamedata_keys.py` → `data/gamedata-keys-1.08/*.txt` 被 2.07 的 key 覆寫，終端機只印「Out dir : …\gamedata-keys-1.08」。
- **嚴重度：** low。
- **確信度：** confirmed。
- **來源：** 既有問題（預設值與吞例外）；2.06 形狀偵測是本次引入（#115）。

### I-15 key snapshot 不涵蓋 bridge 實際讀的表

- **位置：** `scripts/dump_gamedata_keys.py:48-88`（30 張表）；`data/gamedata-keys-2.06/`（30 個檔案，共 99,428 個 key）。對照 bridge 實際讀的表：`src/c_abi/faction_info.rs:480`（`faction`）、`src/c_abi/string_info.rs:291`（`stringinfo`）、以及所有 `*.dlc_01`（`faction.dlc_01`、`factionnode.dlc_01`、`knowledgeinfo.dlc_01`、`characterinfo.dlc_01`、`iteminfo.dlc_01`、`stringinfo.dlc_01`）。
- **問題與根因：** snapshot 的用途是「下個 patch 時 diff 出哪些 key 增減」（`dump_gamedata_keys.py:10-15`、scripts/CLAUDE.md 第 7 步）。`faction`（#116 起有 bridge）與 `stringinfo`（item → dye join 依賴它）不在清單裡；DLC 表一張都沒有（iteminfo 的 DLC 有 `data/keys-2.06-dlc_01.txt`，其他表沒有）。CLAUDE.md 特別強調 DLC 的 row index 不是位置、2.04 的漂移是靠 DLC 表才發現的。
- **重現情境：** 2.07 在 `faction.dlc_01` 增減 row：沒有任何已提交的資料能 diff 出來。
- **嚴重度：** low。
- **確信度：** confirmed。
- **來源：** 既有問題（清單本身）；`faction` 與 DLC 表成為 bridge 輸入是本次引入（#116、#119、#124）。

### I-16 `extract_gamedata_bin.py` 沒有覆寫保護

- **位置：** `scripts/extract_gamedata_bin.py:161`（`--version` 只是標籤）、`:177-184`（`out_dir.mkdir(parents=True, exist_ok=True)`）、`:197-206`（`(out_dir / name).write_bytes(raw)`）、`:65-80`（`list_bin_files` 會收子目錄，例如 2.01 的 `bin/misc`）、`:111-155`（只有 PALOC 有 SHA256 manifest）。
- **問題與根因：** scripts/CLAUDE.md 第 0 步說這個封存是「the step that makes every later question answerable, because the previous version's install is gone the moment Steam patches」，CLAUDE.md 也要求「Tell version binaries apart by hash」。但腳本：
  1. 不檢查 `<out>` 是否已有檔案、不比對 `meta/0.paver` 與 `--version`，直接覆寫。
  2. 把 `bin_dir` 的子目錄攤平寫到 `out_dir / name`，不檢查同名衝突（`bin/x` 與 `bin/misc/x` 會互相覆寫）。
  3. bin 檔沒有寫雜湊或 paver 的 manifest（PALOC 有），版本對應只靠手寫的 `README.txt`。
- **重現情境：** Steam 已更新到 2.07，使用者從終端機歷史重跑上次的 `python scripts\extract_gamedata_bin.py --version 2.06 --paloc eng` → `X:\Crimson Desert\gamedata-bin\2.06\` 被 2.07 的檔案覆寫，2.06 的 ground truth 不可復原（除非另一台 PC 的 G: 也有一份；CLAUDE.md 說兩台的封存不完全相同）。
- **嚴重度：** medium（覆寫的是整個 RE 流程依賴、無法重建的基準資料；觸發只需要一次打錯版本號）。
- **確信度：** confirmed（程式碼沒有任何保護）／plausible（情境）。
- **來源：** 既有問題。

### I-17 `update_ce_table.py` 的 `props_from_bin` 防護不一致

- **位置：** `scripts/update_ce_table.py:171-175`（`r["inventory_info_list"][0]` 無防護）vs `:145`（`load_items` 有 `rec.get("inventory_info_list")` 防護）；`:38`（docstring 範例 `--props-bin "X:/Crimson Desert/gamedata-bin/2.05"`）；`:181-194`（`read_paver` 的候選清單）。
- **問題與根因：**
  1. archived 表裡只要有一個 item 的 `inventory_info_list` 為空，整個更新以 `IndexError` 中止。`load_items` 已經假設這種 item 存在（`:145` 的條件），`props_from_bin` 卻沒有。
  2. `--props-bin` 的用途是「parser 讀不了目前 patch 時，從封存的舊版補屬性」，呼叫的卻是目前安裝的 `crimson_rs.parse_iteminfo_from_bytes`（只讀最新佈局）。docstring 的 2.05 範例在 2.06 parser 下讀不了（2.06 body 列首多了 `[u32 row index]`）。正確的用法只有「封存版本 == 目前 parser 支援的版本」，文件沒有說。
  3. `read_paver` 只試 `D:` 與 `C:` 兩個路徑，不讀 `CRIMSON_GAME_ROOT`，找不到時在 CT 裡寫入 `paver unknown`。
- **重現情境：** `--props-bin` 指向 2.05 封存 → `parse_iteminfo_from_bytes` 例外或讀錯；指向含空 inventory list 的版本 → `IndexError`。
- **嚴重度：** low（CT 寫入在 `:604-611`，例外發生在寫入之前，不會產生半寫的 CT）。
- **確信度：** confirmed（程式碼）／plausible（是否真的有空 list 的 item）。
- **來源：** 既有問題。

### I-18 腳本的 install 路徑與索引

- **位置：** 各自的候選清單：`scripts/export_for_ce.py:59-62`、`extract_gamedata_bin.py:41-44`、`dump_gamedata_keys.py:36-39`、`update_ce_table.py:183-184`；寫死的個人路徑：`scripts/decode_dyeslot_113.py:33`、`decode_worldmap_dds.py:20`（`D:\Github\crimsonforge`）與 `:42`、`dump_ref_112.py:13`、`archive/probe_skill_versions.py:50`（另一個第三方 repo 的路徑）與 `:54`；`scripts/README.md`（索引）。
- **問題與根因：** Rust 測試有 `CRIMSON_GAME_ROOT`，腳本沒有任何一個讀它（`Grep CRIMSON_GAME_ROOT scripts/*.py` 無結果），兩邊的覆寫機制不一致。`scripts/README.md` 的索引沒有列 `extract_gamedata_bin.py`（runbook 第 0 步的工具）、`setup-fork-guards.ps1`、`diff_*.py`、`worldmap_*`、`verify_109_to_110.py` 等。
- **重現情境：** 把 `CRIMSON_GAME_ROOT` 指向保留的舊 install 後跑 `export_for_ce.py`：腳本讀的仍是 `D:\` 的 live install。
- **嚴重度：** nit。
- **確信度：** confirmed。
- **來源：** 既有問題。

### I-19 `setup-fork-guards.ps1` 的涵蓋範圍

- **位置：** `scripts/setup-fork-guards.ps1:15`、`:18`、`:21`、`:25-27`、`:37-55`。
- **問題與根因：**
  1. **原生指令失敗不會停：** `$ErrorActionPreference = 'Stop'` 只作用於 cmdlet；`gh`、`git` 是原生程式，失敗只會設 `$LASTEXITCODE`（PowerShell 7.3+ 要另開 `$PSNativeCommandUseErrorActionPreference` 才會停）。`gh` 沒安裝或沒登入時，`:18` 失敗、腳本繼續、最後照樣印「Fork guards set:」。
  2. **hook 只管 `git push`：** CLAUDE.md 的「Never PR/push to upstream」裡，PR 這一半走的是 `gh` 的 API（`gh pr create --repo potter420/crimson-rs`、`gh repo sync` 等），不經過任何 git hook。腳本只能靠 `:18` 的 `set-default` 降低誤觸機率。
  3. **URL 比對是子字串 `potter420/crimson-rs`**（`:42-47`，已轉小寫）：HTTPS、SSH（`git@github.com:potter420/crimson-rs.git`）、`ssh://…/potter420/crimson-rs`、大小寫變化都擋得住（見文末排除項）；擋不住上游 repo **改名後**的新 URL（GitHub 會對舊名做 redirect，新名不含 `crimson-rs`），也擋不住 `--no-verify`（腳本註解已說明）。
  4. **既有 hook 時只警告：** `:51-52` 遇到不是自己裝的 `pre-push` 就跳過安裝，只印 `Write-Warning`，之後的摘要仍印出 hook 路徑，看起來像已安裝。
- **重現情境：** 在沒有 `gh` 的新機器上執行腳本 → 終端機最後顯示「Fork guards set」，`gh repo set-default` 實際上沒有設定。
- **嚴重度：** low。
- **確信度：** confirmed（程式碼）／plausible（PowerShell 版本行為與上游改名情境，未實測）。
- **來源：** 既有問題。

### I-20 `tools/git-crypt.exe` 沒有來源文件

- **位置：** `tools/git-crypt.exe`；唯一的說明是 `CLAUDE.md:79`（「`tools/git-crypt.exe` stays for them」）。`docs/` 裡（審計報告以外）沒有任何一處提到它；`docs/historical-parser-setup.md:14-15` 與 dev-log #121 只提到 git-crypt 這個機制。
- **問題與根因：** 一個 checked-in 的 Windows 執行檔，沒有記錄上游來源、版本、SHA256，也沒有授權說明（git-crypt 是 GPLv3；散布其二進位檔通常伴隨提供原始碼的義務——此點為一般性認知，未針對該檔驗證其實際版本與授權）。計畫書審計重點 6 要求的「來源與用途是否有文件說明」：**用途有一句話，來源沒有**。另外 `.gitattributes` 已不存在，舊 commit 的 git-crypt filter 設定只能靠使用者自己記得；`docs/historical-parser-setup.md` 的 `git archive` 繞道已經避開這個問題。
- **重現情境：** 新 clone 的使用者要解密舊 commit 的 fixture，無法確認這個 exe 是否就是官方 release。
- **嚴重度：** nit。
- **確信度：** confirmed（文件缺漏）。
- **來源：** 既有問題。

### I-21 依賴稽核

- **位置：** `Cargo.toml:37-48`、`Cargo.lock`；repo 中沒有 `deny.toml`、`.cargo/audit.toml`；`ci.yml` 沒有稽核步驟。
- **問題與根因：** 審計重點 7 要求 `cargo audit` / `cargo deny`；本次沒有執行（需要核准與網路）。靜態盤點：
  - 直接依賴：`pyo3 0.27.2`（optional）、`chacha20 0.10.0`、`lz4_flex 0.11.6`、`flate2 1.1.9`（`miniz_oxide 0.8.9` 後端）、`hmac 0.12.1`、`sha2 0.10.9`；dev：`tempfile 3.27.0`（`src/binary/paz.rs`、`src/c_abi/mod.rs` 的測試有使用）。
  - **兩代 RustCrypto 並存：** `chacha20 0.10` → `cipher 0.5.1` / `crypto-common 0.2.1` / `block-buffer 0.12.0` / `cpufeatures 0.3.0`；`hmac 0.12` + `sha2 0.10` → `digest 0.10.7` / `crypto-common 0.1.7` / `block-buffer 0.10.4` / `cpufeatures 0.2.17`。功能無問題，只是重複編譯與更大的稽核面。
  - 就我所知 PyO3 有一則影響 0.24.1 以前版本的 advisory，0.27.2 不在其範圍內；其餘套件**未查證**，需要在本機跑 `cargo audit` 才能下結論。
- **重現情境：** 不適用（缺流程）。
- **嚴重度：** nit。
- **確信度：** confirmed（缺流程與盤點結果）／未驗證（advisory）。
- **來源：** 既有問題。依 CLAUDE.md 規則，若要在 CI 加入稽核工具或改動依賴，需先詢問維護者。

### I-22 `character_info.rs` 的測試其實是跨子系統的調查 probe

- **位置：** `src/c_abi/character_info.rs:766-8127`。
- **問題與根因：** 計畫書審計重點 3（只記錄，不搬動）。實際計數：**44 個 `#[test]`**（計畫書寫 49），其中 **37 個 `#[ignore = "investigation only — …"]`**，只有 7 個會執行：3 個純單元測試（`:785-864`）、`c_abi_characterinfo_resolve_portrait_kliff`（`:922`）、`_null_args`（`:1145`）、`_empty_bytes_yields_empty_handle`（`:1209`）、`_load_bad_path_returns_io`（`:1224`），再加上檔尾另一個 `mod` 裡的 `c_abi_characterinfo_dlc_merge_live`（`:8084`）。37 個 probe 的主題是存檔 schema、世界地圖 DDS、染色表、陣營表、商店/傭兵/prefab 連結、寶石插槽、深淵之門座標、實體座標等，幾乎都與 characterinfo bridge 無關。
  - **probe 之間有檔案相依：** `_probe_faction_small_tables`（`:6186-6193`）讀 `out/faction_probe/*.pabgh`，這些檔案是 `_probe_faction_pabgh_shapes`（`:5780-5835`）寫的；單獨跑前者會讀不到而跳過。
  - **複製貼上：** `CRIMSON_GAME_ROOT` 的解析在 `src/` 出現 91 次（`character_info.rs` 22 次），而不是呼叫 `gamedata_layout::game_root()`；`slot107` 存檔搜尋在這個檔案裡複製了 18 次（見 I-06）。
  - probe 都寫到相對於 cwd 的 `out/…`（被 `.gitignore` 的 `/out/` 涵蓋），沒有寫入遊戲目錄或存檔目錄（見文末排除項）。
- **重現情境：** 不適用（結構問題）。審計成本上，這個檔案 363.6 KB，超過單次讀取上限。
- **嚴重度：** low。
- **確信度：** confirmed。
- **來源：** 既有問題。

### I-23 `references/store_info.hexpat` 過時且未標版本

- **位置：** `references/store_info.hexpat:8-20`（沒有版本或日期）、`:22-25`（`struct BString { u32 length; char value[length]; }`）、`:77-78`（`u16 store_key; BString name;`）。
- **問題與根因：** 描述的是 2.04 以前的列頭 `[key][len][name]`；2.04 改成 `[len][name][key]`，2.06 把名稱移到 `.staticinfoheader`、body 列首改成 `[key][u32 row index]`，2.06 也在 storeinfo 內聯了 760 個條件（scripts/CLAUDE.md「Inlined conditions」）。檔案裡的 `BString` 是「u32 len + bytes、沒有 NUL」，正好是 CLAUDE.md Rules 裡 `CString` 的定義，而專案的「BString-shape」是**有** NUL 的那一種。
- **重現情境：** 依 CLAUDE.md「Binary Exploration」指示，對 2.06 的 `storeinfo.staticinfobody` 跑這個 pattern → 從第一列就錯位。
- **嚴重度：** nit。
- **確信度：** confirmed（文字）。
- **來源：** 既有問題。

---

## 跨區塊關注點（I 區塊的套用結果）

| 主題 | 結果 |
|---|---|
| 不可信輸入配置上限 | I 區塊的程式碼大多是測試與腳本，輸入是維護者自己的 install。值得一提的只有 `lib.rs:495-496`（`&pamt_data[12..]`，PAMT 短於 12 B 時 panic，測試內可接受）與 `dump_gamedata_keys.py:131-138`（`n` 來自檔案，迴圈次數最多 65,535 / 2³²，但每次迭代都會因越界提早 `struct.error` 跳出，不會長時間卡住）。正式碼的配置上限屬 A–G 區塊。 |
| Panic 面與 abort | 計畫書 C 區塊的 `TailSizeCache` 多 GB 配置會讓整個 `cargo test` 行程死掉（CLAUDE.md 要求 `--test-threads=4`）。CI 的 test 步驟不帶 `--test-threads`，但 CI 不帶 `c_abi`、也沒有 live install，所以碰不到 skill 的 live 路徑；根因交給 C 區塊。測試設施本身的 panic 都是刻意的 assert。 |
| Roundtrip 對稱性 | 測試設施層面的問題是覆蓋範圍：I-05（encoder roundtrip 不讀 fixtures）、I-08（iteminfo roundtrip 只測 base、讀的是最後一次 export）、I-02（DLC merge 測試可能靜默跳過）。 |
| 版本漂移明確失敗 | **I-02 是本區塊與這個主題最直接相關的 finding。** 正確範本已存在（`live_titles.rs:26-33`、`paz.rs:1208-1221`），建議把它提升到 `gamedata_layout` 的共用 helper（修正另開 PR）。I-07 的過時註解會誤導「紅燈 = 版本不符可忽略」。 |
| Feature gate 組合 | 四種組合中 CI 只編 `default`（check）與 `c_abi,python`（clippy），只執行 `default` 的測試（I-01、I-09）；模組層級 `allow(dead_code)` 掩蓋全 feature 下的死碼（I-12）。`src/lib.rs:17-93` 的模組閘門本身一致（每個 `c_abi`-only 表模組都有 `#[cfg(feature = "c_abi")]`；`save/body` 的 `c_abi` 專用匯出在 `save/mod.rs:56-64`、`body/mod.rs:37-43` 也有閘門）。 |
| `docs/c-abi-surface.md` 一致性 | 該文件沒有提到 CI 或測試指令，與 I 區塊無交集。`src/c_abi/` 手寫的 `extern "C" fn crimson_*` 約 304 個（不含 `impl_name_only_bridge!` 等巨集產生的），逐一對照屬 B、D、G 區塊，本報告未重複。 |

## CI 綠燈的實際涵蓋範圍（審計重點 1）

依上述計數（`#[test]` / `#[ignore` 文字計數，不含巨集產生的測試）：

| 類別 | 測試數（約） | CI 是否執行 | CI 上是否有實質內容 |
|---|---:|---|---|
| default build 的合成測試（`binary`、`crypto`、`item_info`、`skill_info`、`string_info`、`save`、`repo_doc_budget` 中不依賴外部檔案者） | ~87 | 是 | 是 |
| `src/lib.rs` 的 21 個非 ignored 測試 | 21 | 是 | **否**：全部依賴 live install（PAPGT/PAMT/PALOC/skill）、`%LOCALAPPDATA%` 存檔或 `crimson-rs-private` fixtures，CI 上都走 skip 分支 |
| 其他 default build 的 live 測試（`binary/paz.rs:922`、`:1205`、`binary/paver.rs:242` 等，以及 `string_info/mod.rs:563`） | ~3–4 | 是 | 否 |
| `c_abi` 與 27 張表模組的全部測試（含純合成的 null-arg / buffer-too-small / CRET / paver 閘門） | ~286 | **否** | — |
| `#[ignore]` probe | 50 | 否 | — |

換句話說，`main` 的必要檢查實際驗證的是：全 feature 能過 strict clippy、bare default 能編譯、以及約 87 個合成測試。Rust 測試沒有「skip」狀態，skip 都是 `eprintln!` + `return`，`cargo test` 的摘要會把它們計為 passed，CI 日誌裡看不出來。

---

## 計畫書待查事項的查證結果

- **(I) `src/lib.rs:117` 的註解寫「parser targets Crimson Desert 1.05」：確認屬實**，列為 I-07；同類的還有 `:151`、`:327`、`:334`、`:560`、`:790`。
- **(I) `src/lib.rs:116` 的 `GAME_DIR` 寫死：確認屬實**，而且影響比計畫書推測的大：除了不讀覆寫，還會和 `gamedata_layout` 的覆寫混用（I-03）。

## 已排除（查過、沒有問題）

1. **測試不會寫入遊戲目錄或存檔目錄。** `src/` 裡所有 `fs::write` / `create_dir_all` / `File::create` 都看過：測試與 probe 只寫相對於 cwd 的 `out/…`（`character_info.rs:2503`、`:4170`、`:5780` 等，皆在 `.gitignore` 的 `/out/` 之下）；`binary/paz.rs:529`、`:560` 是 `PackGroupBuilder`（H 區塊）、`c_abi/mod.rs:4193` 是存檔寫回 API（G 區塊）、`python.rs` 是 Python API（H 區塊），都不是測試設施。live 存檔測試（`lib.rs:530-1021`）只讀不寫，`test_save_write_parse_roundtrip` 的寫回只在記憶體裡。
2. **`CRIMSON_FIXTURES_DIR` 被一致採用。** 所有 fixture 測試（`lib.rs:854`、`:1304`、`:1394`、`c_abi/element_template.rs:997`、`c_abi/item_part_prefab.rs:1808`、`:2188`）都經過 `test_fixtures::save`；它會檢查 `SAVE` magic，非存檔檔案視為缺失（`test_fixtures.rs:30-46`）。被引用的 6 個 fixture 路徑在 `crimson-rs-private/saves/` 下都存在。
3. **`.gitignore` 覆蓋了不該提交的東西：** `/out/`、`*.save`、`*.key`、`/.git-crypt/`、`__pycache__`、`python/**/*.pyd`、`/.venv/`、`/.claude/`、`*.CEM`。`scripts/__pycache__/*.pyc` 在磁碟上存在，但在忽略規則內。
4. **pre-push hook 對常見 URL 寫法有效：** `$2` 先轉小寫再比對 `*potter420/crimson-rs*`，HTTPS、`git@github.com:` SSH、`ssh://` 形式、`.git` 後綴、大小寫變化都會命中；寫入時把 CRLF 轉 LF（`setup-fork-guards.ps1:54`），避免 Git for Windows 的 sh 讀不了 shebang；hook 路徑用 `git rev-parse --git-path hooks`，會遵守 `core.hooksPath`。
5. **CLAUDE.md 大小上限的數值正確：** `LIMIT_BYTES = 40 * 1024 = 40,960`，比較是 `len > LIMIT`，與 CLAUDE.md「≤ 40 KB (40,960 bytes)」一致。目前 4 份 `CLAUDE.md` 都遠低於上限（最大 30,223 B）。
6. **`data/gamedata-keys-2.06/` 的內容合理：** 30 個檔案、99,428 個 key；相對 `2.05.01`（99,418）的差異都很小且方向合理（`itemgroupinfo` +2、`knowledgeinfo` −1、`partprefabdyeslotinfo` +3、`regioninfo` +1、`skill` +3、`stageinfo` +1、`storeinfo` +1），`partprefabdyeslotinfo` 的 1,703 與 dev-log #123 的「1,703 / 1,703 rows」一致，`skill` 的 2,163 與 #115 的 `_probe_skill_entry_failures 2163 / 2163` 一致。工作樹中 2.06 的檔案是 LF、2.05.01 是 CRLF，內容不受影響（CLAUDE.md 已規定以內容比對）。
7. **`update_ce_table.py` 寫入 CT 的流程是安全的：** 混合換行直接拒絕（`:520-524`）、寫入前先備份到 `out/ce-backup/`（`:604-608`）、保留原本的換行風格（`:609`）、`--dry-run` 不寫 CT（`:604`）。
8. **`LiveTitles`（`c_abi/live_titles.rs`）與 `curated_titles_match_live_install`** 的 skip 語意正確：install 不在才 skip，在了但讀不出來就 panic。
9. **`.cargo/config.toml` 的零 DLL 依賴說法**：以依賴分析看沒有原生依賴，說法 plausible（未建置驗證，見 I-09）。
10. **`pyproject.toml`：** `requires-python = ">=3.12"` 與 `abi3-py312` 一致；`features = ["python"]` 與 `Cargo.toml` 的 `python = ["dep:pyo3"]` 一致；`extension-module` 永遠開啟，與 CLAUDE.md 描述相符。版本固定 `0.1.0`、沒有 build stamp 的問題已在 H-15 列出，不重複。
