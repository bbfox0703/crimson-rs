# Code audit — H. Python 綁定與 mod 打包（2026-11-28）

依 [`code-audit-2026-11-28-plan.md`](code-audit-2026-11-28-plan.md) 的 H 區塊做靜態審計。**沒有修改任何程式碼**，沒有建置或執行 crimson-rs（沒有跑 `maturin develop`、`cargo clippy`、`cargo test`，也沒有 import `crimson_rs`），沒有碰 PAPGT trailer：只讀了程式如何原樣保存它，以及哪些路徑會讓它失效。

- **基準：** `dev` @ `e465942`。下文行號都以這個 commit 為準。
- **範圍：** `src/python.rs`、`src/python_traits.rs`、`python/crimson_rs/{__init__.py,__init__.pyi,create_pack.py,pack_mod.py,validate_game_dir.py,enums.py}`，以及只被 `python` feature 用到的 builder：`src/binary/paz.rs:374-684`（`process_file`、`PackGroupBuilder`）、`src/binary/papgt.rs:191-284`（`add_entry`、`to_bytes`）、`src/binary/pamt.rs:402-476`（`to_bytes` / `to_bytes_with_checksum`）、`src/binary/mod.rs:205-323`（`py_binary_struct!` 產生的 Python 轉換）、`src/item_info/{item,structs}.rs` 的 `WritePyValue` 實作、`src/skill_info/mod.rs:170-211`（`SkillData::write`）。為了判斷文件是否過度承諾，也讀了 `README.md`、`docs/api.md`、`docs/archive-format.md:263-288`、`pyproject.toml`、`Cargo.toml`、`.github/workflows/ci.yml`、`src/lib.rs:1-167`。
- **方法：** 逐行閱讀。來源歸屬用 `git log -S`（pickaxe）找引入的 commit（本 session 的 `git blame` 需要額外核准，沒有執行）。
- **跟其他區塊報告的分工：** A 區塊已列的 builder／容器問題（A-01 trie 遞迴、A-02 依宣告值配置、A-04 `encrypt_info` 寫死、A-10 roundtrip 不對稱、A-15 Python-only builder 的截斷），F 區塊已列的存檔 Python 入口 abort（F 報告 `toc_count`）與 `hmac_ok`，C 區塊已列的 `buff_raw_fallback` 靜默退化，本報告只在「Python 端多出來的後果」處交叉引用，不重複列。

### 這次沒做到的資料比對

1. 我想用一段不經過 crimson-rs、從 stdin 執行的唯讀 Python 解碼 live install 的 `meta/0.papgt`（39 個 entry 的名稱、名稱緩衝區有沒有沒被引用的位元組）。這個指令在本 session 需要額外核准而沒有執行。H-05、H-06 裡關於 live PAPGT 的推論只用了檔案大小（735 B）與目錄清單，都標 plausible。
2. 沒有實際編譯 `--features python`（只開 `python`、不開 `c_abi`）這個組合，H-18 對它能否編譯的判斷是讀碼推論。
3. 沒有用 Python 實際跑 `serialize_iteminfo` 對 2.06 live 表的 roundtrip，所以 H-11 列的「Python 寫回路徑與 Rust `BinaryWrite` 不同」的地方在真實資料上會不會命中，都是 plausible。

### 確信度與來源歸屬的定義

沿用 A、B 區塊報告的定義：

- **confirmed**：只用原始碼或文件文字就能確定的事實，例如「沒有這個檢查」、「沒有這組測試」、「文件和程式碼不一致」。
- **plausible**：行為上的後果是追程式路徑推出來的，**沒有用位元組或實際重現驗證過**。依 CLAUDE.md 與計畫書，這類一律標 plausible，即使程式路徑是確定性的。
- **本次引入**：引入的 commit 在 2.06 移植週期內（`95fa25f`，2026-11-27，到基準 `e465942`）。H 區塊在這段期間只有 `95fa25f` 動過 `src/python.rs`、`.pyi`、`binary/types.rs`（`HeaderName`）與 `item_info/item.rs`。
- **既有問題**：引入早於 `95fa25f`。主要來源：`8623be0`（2026-03-28，打包、PAPGT/PAMT dict 往返、`pack_mod` / `create_pack` / `validate_game_dir`）、`ea9263f`（2026-03-29，`python_traits` 巨集化）、`7aac33b`（2026-05-08，`inspect_legacy_patches`）、`dc0b6a3`（2026-05-07，lossy parser）、`3188491` 起的存檔 Python 入口（2026-05-12）、`0694dfb`（2026-06-19，條件欄位 `=>`）、`1f32ec2`（2026-11-01，2.04：`EnchantDataList` 的 Python 寫回、`[u16; 12]`）。

---

## 摘要

| # | 標題 | 嚴重度 | 確信度 | 來源 |
|---|---|---|---|---|
| H-01 | `inspect_legacy_patches` 在 2.06 表上對每個 patch 都回 `None`：不收 header，`string_key` 全是 `""`，用名稱建的索引查不到任何 entry | medium | confirmed（程式碼）／plausible（後果） | 本次引入 |
| H-02 | `serialize_iteminfo` / `write_iteminfo_to_file` 只輸出 body，沒有任何 Python API 能重建 2.06 `.staticinfoheader`；README 範例的改長度編輯會產生與原 header 不符的 body，`string_key` 的編輯被靜默丟掉 | medium | confirmed（缺 API）／plausible（後果） | 本次引入 |
| H-03 | `serialize_pamt` / `write_pamt_file` 沿用 dict 裡的舊 header checksum：編輯後的 PAMT 連 crimson-rs 自己的 `PackMeta::parse` 都會拒絕；`pamt.rs` 的註解與 `parse` 的行為相反 | low | confirmed（程式碼）／plausible（後果） | 既有問題 |
| H-04 | PAMT dict 往返以「目錄清單 × 各自檔案」攤平重建 `raw_files`，不保留原順序與孤兒檔；`path` / `name` / `compression` / `crypto` 的編輯被靜默忽略 | low | confirmed（程式碼）／plausible（真實資料是否命中） | 既有問題 |
| H-05 | PAPGT dict 往返從 entries 重建名稱緩衝區：offset 重疊時名稱互相覆寫、未引用的位元組消失、超大 offset 觸發 GB 級 `resize` | low | confirmed（程式碼）／plausible（後果） | 既有問題 |
| H-06 | `pack_mod` / `create_pack_group*` 不拒絕已在 PAPGT 裡的 `group_name`：upsert 會換掉原版 entry；文件範例的 `"0036"` 在這台的 2.06 install 上很可能就是原版 entry | medium | confirmed（缺檢查）／plausible（`0036` 碰撞） | 既有問題 |
| H-07 | `create_pack_group*` 與 `validate_game_dir` 就地改寫 live `meta/0.papgt`：沒有備份、非原子寫入；`validate_game_dir` 在 2.04+ 上讓 trailer 失效卻完全沒有警告；`archive-format.md` 說「原版封存永遠不被修改」 | medium | confirmed（程式碼）／plausible（遊戲端後果） | 既有問題 |
| H-08 | 打包中途失敗會留下半成品群組目錄，之後重跑一律 `FileExistsError` | low | confirmed（程式碼） | 既有問題 |
| H-09 | `group_name` / `output_dir` 沒有驗證：`..`、絕對路徑可寫到預期目錄之外；`PackGroupBuilder` 直接覆寫既有的 `N.paz` / `0.pamt` | low | confirmed（程式碼）／plausible（後果） | 既有問題 |
| H-10 | `[f32; 3]`、`[u32; 2]`、`[u32; 4]` 的 `WritePyValue` 不檢查長度，長度錯的 list 會寫出錯位的 row，不報錯 | low | confirmed（程式碼）／plausible（後果） | 既有問題 |
| H-11 | Python 寫回路徑和 Rust `BinaryWrite` 不是同一份實作，而且完全沒有自動化測試；README 與 `scripts/CLAUDE.md` 的「`serialize_iteminfo` roundtrips every item」沒有任何測試在守 | low | confirmed（缺測試）／plausible（差異在真實資料上命中） | 既有問題 |
| H-12 | 條件欄位（`unk_pre_gimmick_visual`）寫回時不重算條件：只改 `equip_type_info` / `item_type` 會寫出讀不回來的 row | low | confirmed（程式碼）／plausible（後果） | 既有問題 |
| H-13 | `.pyi` 與實際 `#[pyfunction]` 脫節：缺 6 個函式、`ItemInfo` TypedDict 有 7 個已不存在的欄位且缺 9 個欄位、多個巢狀結構缺欄位、沒有 `py.typed` | low | confirmed | 既有問題（`row_index` 部分本次已補） |
| H-14 | `docs/api.md` 等文件與程式脫節：`Language` 表全錯、`Crypto.ICE/AES` 宣稱可用但會被拒、「105 fields」、已刪的欄位、過時的封存路徑範例；`pack_mod` 章節沒有 2.04+ 警語 | low | confirmed | 既有問題 |
| H-15 | 沒有任何版本自檢：沒有 `__version__` 或 build stamp（版本永遠 `0.1.0`），Python 沒有 paver／`PARSER_TARGET_*` 介面，stale `.pyd` 在執行期無法偵測 | low | confirmed | 既有問題 |
| H-16 | 2.06 的 lossy 掃描錨點變弱且不收 header；`parse_iteminfo_tracked` / `_lossy` 在 2.06 一律 `string_key == ""`；被封鎖（`is_blocked != 0`）的列不可能被錨點救回 | low | confirmed（程式碼）／plausible（誤錨率） | 本次引入（錨點）／既有（`is_blocked` 條件） |
| H-17 | Python 可達的 abort／`PanicException` 面：`decompress_data` 的 `uncompressed_size` 是 `usize`，可直接要求任意大小的配置；`PanicException` 不是 `Exception` 子類別 | low | plausible | 既有問題 |
| H-18 | Feature gate 與死碼：只開 `python` 的組合 CI 從不編譯；Python 綁定沒有任何測試目標；`[u8; 22]` / `[u8; 9]` 的 Python 實作與 `PrefabData` / `GimmickVisualPrefabData` 是死碼但 lint 看不到 | nit | confirmed（CI／死碼）／plausible（能否編譯） | 既有問題 |
| H-19 | `serialize_skillinfo` 缺 `row_indices` key 時靜默寫出 pre-2.06 佈局；`index_order[i].key` 與 `entries[i].key` 不一致時不報錯 | nit | confirmed（程式碼） | 本次引入（`row_indices`）／既有（key） |

沒有 critical 或 high。H 區塊的問題集中在兩類：

1. **2.06 把名稱搬進 header 之後，Python 這層只改了「讀」的一半。** `parse_iteminfo_from_*` 收 header，但 `inspect_legacy_patches`、`parse_iteminfo_tracked`、`parse_iteminfo_lossy` 不收（H-01、H-16），寫回也沒有 header 的對應物（H-02）。這三條都是 `95fa25f` 引入。
2. **打包腳本對使用者的遊戲安裝目錄太信任。** 不檢查群組名稱碰撞（H-06）、就地改 live PAPGT 而且 `validate_game_dir` 完全沒有 trailer 警告（H-07）、失敗時不清理（H-08）。依 CLAUDE.md，2.04+ 的打包輸出本來就不保證能載入，這幾條講的是「即使不談能不能載入，它也可能把原版安裝弄壞」。

dict → Rust 的轉換本身是健康的：缺 key 一律 `PyKeyError`（`python.rs:16-28`、`python_traits.rs:23-26`），型別錯誤是 `TypeError`（`cast`）或 `OverflowError`（`extract`），沒有找到會 panic 的路徑（見文末「已排除」）。

---

## Findings

### H-01 — `inspect_legacy_patches` 在 2.06 表上對每個 patch 都回 `None`

- **位置：** `src/python.rs:263-347`（建索引 `:275-294`，key 取自 `item.string_key.data` `:290`，查詢 `:307`）；`src/binary/types.rs:95-127`（`HeaderName` 的 `read_from` / `read_tracked` 回傳預設值 `""`）；`src/item_info/item.rs:525`（`string_key: HeaderName<'a>`）。文件：`docs/api.md:316-348`、`python/crimson_rs/__init__.pyi:741-764`。
- **問題與根因：**
  1. 2.06 起 row 不含名稱，`ItemInfo::read_tracked` 對 `string_key` 一律回 `""`（`types.rs:118-126`）。`inspect_legacy_patches` 沒有 header 參數，所以 7,068 列的 `string_key` 全是 `""`，`index` 只有一個 key `""` → 第 0 列（`:292` 是 `or_insert`，first wins）。
  2. 任何真實的 `entry` 名稱（例如文件範例的 `"Pyeonjeon_Arrow"`）都在 `:307` 查不到，回 `None`，跟「entry 不存在」的回傳值一模一樣。呼叫端分不出「名稱不在表裡」和「這個函式在 2.06 上根本不能用」。違反跨區塊的「版本漂移要明確失敗」。
  3. 反過來，`entry: ""` 會被歸到第 0 列，給出一個看似有效、其實無意義的答案。
  4. 另外，legacy patch 的 `rel_offset` 是相對於 pre-2.06 row 起點（含 `[u32 len][name]`）算的，2.06 的 row 起點少了名稱，就算查得到名稱，offset 也全部偏移 `4 + len(name)` 再加上 row index 的 4 B。這點文件沒有提。
- **可重現的情境：** 從 2.06 install 抽出 `iteminfo.staticinfobody`，執行 `crimson_rs.inspect_legacy_patches(body, [{"entry": "Pyeonjeon_Arrow", "rel_offset": 0}])`。依程式路徑預期回傳 `[None]`；`docs/api.md:329-333` 的範例輸出（第一個 hit 是 `key`）不會出現。
- **嚴重度：** medium（整個函式在目前唯一支援的版本上靜默失效）。
- **確信度：** confirmed（程式碼：沒有 header 參數、索引鍵必為 `""`）／plausible（實際回傳值沒有執行驗證）。
- **來源：** 本次引入（`95fa25f` 把 `string_key` 改成 `HeaderName`；函式本身來自 `7aac33b`）。
- **建議方向（不在本次修改）：** 加 `header` 參數，走 `static_header::parse_named_header` 取名稱與列邊界；沒給 header 而偵測到 2.06 body 時回 `ValueError`，不要回全 `None`。

### H-02 — Python 寫回 iteminfo 只有 body，沒有 header

- **位置：** `src/python.rs:349-367`（`write_iteminfo_to_file` / `serialize_iteminfo`）、`src/python_traits.rs:231-236`（`HeaderName` 寫回 0 bytes）、`src/python.rs:1891-1926`（註冊清單裡沒有任何產生 `.staticinfoheader` 的函式）。對照：Rust 端有 `static_header::from_legacy_layout`（`src/binary/static_header.rs:180-224`），skill 的寫回有用到（`skill_info/mod.rs:199-208`），iteminfo 的 Python 路徑沒有。文件：`README.md:16`、`:139-148`（「Write modified items back to a file (byte-perfect roundtrip)」，範例改了 `item_name.default`）、`docs/api.md:391-418`、`.pyi:767-791`。
- **問題與根因：**
  1. 2.06 的 `.staticinfoheader` 每列記 `[key][len][name][offset][size]`。只要任何一列的長度改變（README 範例的 `item["item_name"]["default"] = "Custom Name"` 就會），之後每一列的 offset 都要跟著變。`serialize_iteminfo` 只吐 body，原 header 的 offset / size 全部過期。
  2. crimson-rs 自己的 `parse_iteminfo_with_header`（`item.rs:689-720`）會拿新 body 配舊 header 失敗（`row … runs past the body` 或 `n of m bytes read`）；遊戲端會怎樣沒有驗證。
  3. `string_key` 的編輯被靜默丟掉（`HeaderName` 寫回 0 bytes），沒有任何錯誤或警告。
  4. 名稱長度不變、只改定長欄位的編輯不受影響。
- **可重現的情境：** `items = parse_iteminfo_from_file(body, header_path=hdr)`；把某列的 `item_memo` 加長一個字元；`write_iteminfo_to_file(items, "out.body")`；再 `parse_iteminfo_from_file("out.body", header_path=hdr)`。依程式路徑預期第一個被影響的列之後報 `ValueError`。
- **嚴重度：** medium（README 主打的「編輯後寫回」流程在 2.06 上產出不自洽的表，而且沒有 API 能補）。
- **確信度：** confirmed（缺 API、`HeaderName` 寫回 0 bytes）／plausible（解析失敗的具體訊息沒有執行驗證）。
- **來源：** 本次引入（`95fa25f`）。
- **建議方向：** 提供 `serialize_iteminfo_with_header(items) -> (header, body)`，或至少讓 `serialize_iteminfo` 在 2.06 回傳 header；README 範例同步更新。

### H-03 — `serialize_pamt` 沿用舊的 header checksum

- **位置：** `src/python.rs:603-689`（`:606` 從 dict 讀 `checksum`，`:687` 呼叫 `to_bytes`）；`src/binary/pamt.rs:440-453`（`to_bytes` 原樣寫 `header.checksum`，`:447-448` 的註解說「the actual post-header checksum is validated by the papgt entry, not stored in the pamt header itself」）；對照 `pamt.rs:208-213`（`parse` **一定**驗證 header checksum）與 `:455-476`（`to_bytes_with_checksum`，Python 沒用）。
- **問題與根因：**
  1. PAPGT 的 dict 寫回會重算 checksum（`papgt.rs:265-280`），PAMT 的不會。對 PAMT dict 做任何會改變 post-header 位元組的編輯（chunk size、某檔的 `compressed_size`、`flags`、名稱緩衝區），輸出的 header checksum 都是舊值。
  2. `PackMeta::parse` 會用 header checksum 驗證 post-header（`:212`），所以 `parse_pamt_bytes(serialize_pamt(edited))` 會失敗。`pamt.rs:447-448` 的註解與 `parse` 的實際行為互相矛盾。
  3. `.pyi:979-993` 與 `docs/api.md:136-142` 都沒說呼叫端要自己重算。`validate_game_dir.py:50-66` 只會改 PAPGT 的 `pack_meta_checksum`，不會修 PAMT header。
- **可重現的情境：** `d = parse_pamt_file("0019/0.pamt")`；`d["chunks"][0]["size"] += 1`；`b = serialize_pamt(d)`；`parse_pamt_bytes(b)`。依程式路徑預期 `ValueError`（checksum mismatch）。
- **嚴重度：** low。
- **確信度：** confirmed（程式碼）／plausible（沒有執行）。
- **來源：** 既有問題（`8623be0`）。
- **建議方向：** dict 路徑改用 `to_bytes_with_checksum`，或明確提供 `recompute_checksum` 參數；修正 `pamt.rs:447-448` 的註解。

### H-04 — PAMT dict 往返不保留檔案順序與孤兒檔，部分欄位編輯被忽略

- **位置：** `src/python.rs:503-573`（`to_py_pamt` 只輸出「已解析目錄 → 它範圍內的檔案」，沒有 `raw_files`）、`:627-658`（`wr_pamt_from_dict` 依目錄清單順序把各目錄的 `files` 串接成 `raw_files`，`file_start_index` / `file_count` 卻直接取 dict 值）；`src/binary/pamt.rs:299-388`（`directories` 依 `raw_directories` 順序、取 `raw_files[start..end]`）。
- **問題與根因：**
  1. 只有在「每個目錄的檔案範圍依目錄順序連續排列、沒有重疊、沒有不屬於任何目錄的檔案」時，攤平重建才會跟原 `raw_files` 一致。不符合時，`raw_files` 被重排或丟檔，而 `file_start_index` 沒有重算，目錄就指到別人的檔案。Rust 端的 live roundtrip 測試（`src/lib.rs:223-232`）走的是 `PackMeta::to_bytes`，保留 `raw_files`，測不到這條。
  2. 在 dict 的某個目錄 `files` 加一筆或刪一筆，`file_count` / 後面目錄的 `file_start_index` 不會跟著調整，也不報錯。
  3. `path`（目錄）、`name`（檔案）、`compression`、`crypto`、`is_partial` 這幾個解碼欄位在寫回時完全不讀；名稱由 `_dir_names_buffer` / `_file_names_buffer` 決定，壓縮與加密由 `flags` 決定。改了沒效果，也沒有警告。`.pyi:939-976` 沒標示哪些是唯讀欄位。
- **可重現的情境：** 構造一個 PAMT：兩個目錄，`raw_directories` 順序是 B（`file_start_index = 1`）、A（`file_start_index = 0`），各一個檔案，重算 checksum。`serialize_pamt(parse_pamt_bytes(x))` 會把 B 的檔案放到 index 0、A 的放到 index 1，但兩個目錄的 `file_start_index` 不變，於是互換了檔案。（另：`parse` 不檢查 `name_offset` 指向的目錄字串，checksum 是 `hashlittle`，重算容易。）
- **嚴重度：** low（真實 PAMT 是否符合「連續、依目錄順序」沒有驗證；builder 產出的 PAMT 符合）。
- **確信度：** confirmed（程式碼）／plausible（真實資料是否命中）。
- **來源：** 既有問題（`8623be0`）。

### H-05 — PAPGT dict 往返從 entries 重建名稱緩衝區

- **位置：** `src/python.rs:401-471`（`:412` 空緩衝區、`:426-432` 依各 entry 的 `group_name_offset` 填入、`:459` `entries.len() as u8`）；對照 `src/binary/papgt.rs:169-181`（parse 保留原緩衝區）與 `:191-240`（Rust `add_entry` 是 prepend 而不是依 offset 寫）。
- **問題與根因：**
  1. **offset 重疊靜默覆寫：** 使用者在 dict 裡 append 一個 entry 並沿用某個既有 entry 的 `group_name_offset`（很自然的「複製一筆再改名」），後寫入者覆蓋前者的名稱位元組。輸出的 PAPGT 有兩筆同名 entry，原名稱消失，不報錯。`.pyi:862-875` 只說它是「offset in the group names buffer」，沒說必須唯一、也沒指向 `add_papgt_entry`。
  2. **未引用位元組消失：** 原緩衝區裡不被任何 entry 引用的位元組（例如 Rust `add_entry` upsert 之後留下的舊名稱）在 dict 往返後變成 0 或被截掉，所以 `serialize_papgt(parse_papgt_bytes(x)) == x` 只在緩衝區被完全引用時成立。live 2.06 的 `0.papgt` 是 735 B，若 39 個 entry 都是 4 字元名稱，`4 + 39×12 + 4 + 39×5 + 64 = 735` 剛好吻合，所以**目前 live 檔很可能完全被引用**（沒有解碼驗證）。
  3. **超大 offset：** `group_name_offset = 0xFFFFFFFF` 會讓 `:428` `resize` 到約 4 GiB；配置失敗時行程 abort（`PanicException` 也救不了，見 H-17）。只有手寫的 dict 會走到這裡，`parse_papgt_bytes` 解出的 offset 都在緩衝區內。
  4. `entry_count` 的 u8 wrap 見 A-15。
- **可重現的情境：** `d = parse_papgt_file(...)`；`e = dict(d["entries"][0]); e["group_name"] = "9999"; d["entries"].append(e)`；`parse_papgt_bytes(serialize_papgt(d))`。依程式路徑預期第 0 筆與新加的一筆都叫 `"9999"`，原本的 `"0000"` 不見。
- **嚴重度：** low。
- **確信度：** confirmed（程式碼）／plausible（後果沒有執行；live 緩衝區是否完全被引用只用算術推論）。
- **來源：** 既有問題（`8623be0`）。

### H-06 — 打包腳本不拒絕已存在於 PAPGT 的群組名稱

- **位置：** `python/crimson_rs/pack_mod.py:36-38`（只檢查 `output_dir/<group>` 是否存在）、`:87-93`（`add_papgt_entry`）、`:107-109`（安裝指示：把輸出群組「copy into the game directory」）；`python/crimson_rs/create_pack.py:101-110`（只檢查 `game_dir/<group>` 目錄是否存在）、`:132-138`；`src/binary/papgt.rs:191-209`（`add_entry` 是 upsert：`retain` 掉同名 entry 再插到最前面）。文件範例都用 `"0036"`：`docs/api.md:64`、`:152`、`.pyi:1061`、`:1102`、`src/python.rs:859`。
- **問題與根因：**
  1. 兩個腳本都有原版 PAPGT 的 entry 清單，卻沒有檢查 `group_name` 是否已在其中。名稱碰撞時 `add_entry` 會**刪掉原版 entry**，換成 mod 的 checksum 並移到最前面。
  2. `pack_mod` 的檢查對象是 `output_dir/<group>`，所以就算 `game_dir/<group>` 是原版群組（例如 `"0008"`），只要輸出目錄是空的就會通過；照它最後印的指示把輸出複製回遊戲目錄，會覆寫原版的 `0.pamt`、`0.paz`，而原版其他 `N.paz` 留在原地。
  3. `create_pack` 檢查 `game_dir/<group>` 目錄，擋得住「目錄存在」的原版群組，擋不住「PAPGT 有 entry、目錄不存在」的情況。
  4. **文件範例的 `"0036"` 很可能就是這種情況。** 這台的 2.06 install 根目錄有 35 個數字目錄（`0000`–`0017`、`0019`–`0033`、`0035`、`0038`），沒有 `0018`、`0034`、`0036`、`0037`；`0.papgt` 735 B 若照 H-05 的算術是 39 個 entry。39 − 35 = 4，正好對上這四個缺的編號，所以 `0036` 很可能是一個原版（或其他 mod 工具加入的）entry，只是沒有對應目錄。目錄裡也有 `mods`、`CDMods`，PAPGT 可能已被其他工具改過，所以這只是推論。
- **可重現的情境：** 在 2.06 install 上照 `docs/api.md:60-68` 執行 `pack_mod(..., group_name="0036")`。若 `0036` 確實在原版 PAPGT 裡，輸出的 `meta/0.papgt` 會少掉原本那筆 `0036`，換成 mod 的。用 `create_pack_group_from_folder(game_dir, "0036", …)` 則會直接改 live PAPGT。
- **嚴重度：** medium（照文件範例操作就可能換掉原版 entry；Steam「驗證檔案」可以救回）。
- **確信度：** confirmed（缺檢查、upsert 語意）／plausible（`0036` 是否真的在 live PAPGT 裡沒有解碼驗證）。
- **來源：** 既有問題（`8623be0`）。
- **建議方向：** `group_name` 已在 PAPGT 或 `game_dir/<group>` 存在時拒絕（給一個明確的 `overwrite=` 開關）；文件範例改用「目前最大編號 + 1」的算法。

### H-07 — 就地改寫 live `meta/0.papgt`；`validate_game_dir` 沒有 trailer 警告

- **位置：**
  - `python/crimson_rs/create_pack.py:113-141`（`_update_papgt` 讀 `game_dir/meta/0.papgt`，`:139` 寫回同一個檔；`:126-131` 有 trailer 警告）。
  - `python/crimson_rs/validate_game_dir.py:10-74`（`:65` 改 checksum，`:70-72` 寫回 live PAPGT；**整個檔案沒有 trailer 檢查或警告**，也沒有 dry-run 或確認）。
  - `src/python.rs:489-493`（`write_papgt_file` 用 `std::fs::write`：先截斷再寫）。
  - `docs/archive-format.md:272`（「The original game archives are never modified」）。
- **問題與根因：**
  1. CLAUDE.md 要求不得宣稱 2.04+ 的 `pack_mod` 輸出能被載入。`pack_mod` / `create_pack` 都有印警告（`pack_mod.py:95-100`），`validate_game_dir` 沒有：它在 2.04+ 上只要發現任一群組的 PAMT checksum 不符就改寫 live PAPGT，trailer 從此不再對應內容，訊息只有「Wrote updated papgt」。它的用途正是「修好被改過的群組」，在有 `mods` / `CDMods` 的安裝上很容易觸發。
  2. `create_pack_group*` 與 `validate_game_dir` 都直接改遊戲目錄裡唯一的一份 `0.papgt`，沒有備份；`std::fs::write` 不是原子寫入，中途中斷就留下截斷的檔案。
  3. `archive-format.md:272` 說原版封存永遠不被修改，但 `0.papgt` 本身就是原版檔案，`create_pack` 與 `validate_game_dir` 都會改它。
- **可重現的情境：** 在 2.06 install 上任一群組的 `0.pamt` 與 PAPGT 記錄的 checksum 不符時（例如被其他 mod 工具改過），執行 `python -m crimson_rs.validate_game_dir <game_dir>`。依程式路徑：印出 `UPDATE …`，然後覆寫 `meta/0.papgt`，保留舊 trailer，沒有任何關於 trailer 的訊息。遊戲是否因此拒絕啟動**沒有驗證**（依規則也不去驗證）。
- **嚴重度：** medium（直接改動使用者的遊戲安裝，而且在 2.04+ 上是唯一不警告的入口）。
- **確信度：** confirmed（程式碼與文件文字）／plausible（遊戲端後果）。
- **來源：** 既有問題（`8623be0`；trailer 警告是 2.04 週期只加到另外兩個腳本）。
- **建議方向：** `validate_game_dir` 加上與另兩個腳本相同的警告，預設 dry-run；所有改 live PAPGT 的路徑先寫 `0.papgt.bak`，再用「寫暫存檔 + rename」。修正 `archive-format.md:272`。

### H-08 — 打包中途失敗會留下半成品目錄，之後重跑被擋

- **位置：** `python/crimson_rs/create_pack.py:26-41`（`PackGroupBuilder(...)` 在 `src/python.rs:886` 就 `create_dir_all`，`count == 0` 的 `ValueError` 在建立目錄**之後**）、`:68-94`；`python/crimson_rs/pack_mod.py:40-42`（先 `mkdir` 群組與 `meta` 目錄，`:72-73` 才檢查 `count == 0`）；`create_pack.py:109-110` / `pack_mod.py:37-38`（目錄已存在就 `FileExistsError`）。
- **問題與根因：** 「建立輸出目錄」發生在所有會失敗的步驟之前，失敗時不清理。`files={}`、空的 mod 資料夾、`add_file_from_path` 讀檔失敗（權限、檔案被鎖）、`add_papgt_entry` 失敗，都會留下空的或含部分 `N.paz` 的群組目錄。下一次用同一個 `group_name` 重跑會直接 `FileExistsError`，使用者得手動刪除；`create_pack` 的情況下這個殘留目錄在**遊戲目錄**裡。
- **可重現的情境：** `create_pack_group(game_dir, "0099", files={})` → `ValueError("No files provided")`，`game_dir/0099/` 已被建立；再跑一次 → `FileExistsError`。
- **嚴重度：** low。
- **確信度：** confirmed（程式碼順序）。
- **來源：** 既有問題（`8623be0`）。

### H-09 — 路徑參數沒有驗證；`PackGroupBuilder` 直接覆寫既有檔案

- **位置：** `python/crimson_rs/create_pack.py:21-24`、`:59-66`（`game_path / group_name`）；`pack_mod.py:36`（`out_path / group_name`）；`src/python.rs:871-899`（`PackGroupBuilder.new` 對任何 `output_dir` 都 `create_dir_all`）；`src/binary/paz.rs:526-529`、`:559-560`（`std::fs::write` 直接覆寫 `{id}.paz` / `0.pamt`）；讀取端 `src/python.rs:990`（`extract_file` 的 `game_dir.join(group_name)`）。
- **問題與根因：**
  1. `group_name` 同時是檔案系統路徑片段與 PAPGT 裡的名稱，卻沒有格式檢查。`"../x"` 會把群組寫到遊戲目錄外，`"C:/Temp/x"` 這種絕對路徑在 `pathlib` 的 `/` 下會**取代**整個前綴；兩者都會原樣寫進 PAPGT 的名稱緩衝區。
  2. `PackGroupBuilder` 這個公開類別本身沒有任何「目錄已存在」的檢查。直接指向原版群組目錄（`.pyi:1061` 的範例就是 `"/path/to/0036"`）會覆寫 `0.paz` / `0.pamt`，原版其他 `N.paz` 留下。
  3. 讀取端（`extract_file`）的穿越只是讀檔，影響小。
  4. 這些都是呼叫端自己給的參數，不是不可信的封存內容，所以嚴重度低。
- **可重現的情境：** `create_pack_group(game_dir, "../evil", {"a": {"b.txt": b"x"}})` → 在遊戲目錄的上一層建立 `evil/`，live PAPGT 多一筆名稱為 `../evil` 的 entry。
- **嚴重度：** low。
- **確信度：** confirmed（沒有驗證）／plausible（`pathlib` 合併結果沒有執行驗證）。
- **來源：** 既有問題（`8623be0`）。

### H-10 — 定長陣列的 Python 寫回不檢查長度

- **位置：** `src/python_traits.rs:78-87`（`[f32; 3]`）、`:95-104`（`[u32; 2]`）、`:139-148`（`[u32; 4]`）。對照有檢查的 `[u16; 12]`（`:114-131`）、`[u8; 22]`（`:163-177`）、`[u8; 9]`（`:190-204`）。使用處：`src/item_info/structs.rs:456`（`MergedPrefabVisualData.scale`）、`:488` / `:510`（`DockingChildData.docking_tag_name_hash` / `summon_tag_name_hash`）、`:578`（`PatternParamString.unk_value`）。
- **問題與根因：** 這三個實作把 list 裡有幾個元素就寫幾個，不看長度。`scale` 少給一個 float，row 就少 4 B，後面每個欄位都錯位；`serialize_iteminfo` 不報錯，錯誤要到重新解析（或遊戲讀取）時才會以別的形式出現。2.03 / 2.04 為 `[u16; 12]` 補長度檢查時（`:117-118` 的註解說明了原因）沒有一併補這三個。
- **可重現的情境：** 取一個 `merged_prefab_visual_list` 非空的 item，把 `scale` 設成 `[1.0, 1.0]`，`serialize_iteminfo([item])`，再用 `parse_iteminfo_from_bytes` 讀回。依程式路徑預期該 item 讀取失敗或讀出錯位的值。
- **嚴重度：** low。
- **確信度：** confirmed（程式碼）／plausible（後果）。
- **來源：** 既有問題（`ea9263f`）。

### H-11 — Python 寫回路徑是另一份實作，而且沒有測試

- **位置：**
  - `src/item_info/structs.rs:290-307`（`EnchantDataList` 的 Python 寫回一律寫 flag `1`，separator 一律寫 `0`）vs `:181-199`（Rust `BinaryWrite` 保留原 flag 與 separator）。
  - `src/python_traits.rs:328-342`（`CFlaggedArray` 空 list 寫 `00`）、`:355-365`（`COptional` 非 None 一律寫 flag `1`）、`:214-221`（`CString` 寫 Python `str` 的 UTF-8）；對照 `src/binary/types.rs:84-91`（非 UTF-8 的 `CString` 讀成 lossy 字串，Rust 端靠 `raw` 原樣寫回，Python 端拿不到 `raw`）。
  - `src/python.rs:743-770`（`serialize_paloc` 同樣只寫 lossy 字串）；`docs/api.md:245-246`（「Byte-identical to the entry list they were parsed from」）。
  - 測試：`src/python.rs`、`src/python_traits.rs` 沒有任何 `#[cfg(test)]`；repo 裡沒有 `test*.py`；`.github/workflows/ci.yml:57-58` 只跑 `cargo test --lib`（不含 `python` feature）。`src/lib.rs:149-167` 的 iteminfo roundtrip 測的是 Rust `write_to`，不是 `write_from_py_dict`。
- **問題與根因：**
  1. 同一個型別有兩套寫回：Rust `BinaryWrite` 和 Python `WritePyValue`。後者在上面這幾種輸入下會產生不同的位元組：`EnchantDataList` flag 為 0 的列（`00` → `01 00000000`）、非零 separator、`CFlaggedArray` 的 `01` + count 0（`01 00000000` → `00`）、`COptional` flag ≥ 2（見 A-10）、非 UTF-8 字串（lossy 後長度改變）。
  2. 這些差異在目前真實資料上會不會出現沒有驗證。`structs.rs:133-134` 說 2.04 的 enchant 清單「empty 時 flag 仍是 1」，所以第一項很可能不會命中；其餘幾項不清楚。
  3. 真正的問題是沒有任何測試守住 Python 路徑。`README.md:183` 與 `scripts/CLAUDE.md:7` 寫「`serialize_iteminfo` roundtrips every item」，但 repo 裡沒有任何自動化檢查在證明這句話。
- **可重現的情境：** 構造一列 `enchant_data_list` flag 為 `00` 的 item（或一個含非 UTF-8 位元組的 `item_memo`），`serialize_iteminfo(parse_iteminfo_from_bytes(x)) != x`。
- **嚴重度：** low。
- **確信度：** confirmed（兩套實作的差異、缺測試）／plausible（真實資料是否命中）。
- **來源：** 既有問題（`EnchantDataList` 寫回來自 `1f32ec2`，其餘來自 `ea9263f`）。
- **建議方向：** 讓 `WritePyValue` 透過「dict → Rust 結構 → `BinaryWrite`」實作，只留一套寫回；或至少加一個 `python` feature 下的測試，在 live 資料上比對兩條路徑。

### H-12 — 條件欄位寫回時不重算條件

- **位置：** `src/binary/mod.rs:224-236`（`__pbs_write_py!` 的條件分支：值為 `None` 就不寫，否則就寫，不看條件）、`:143-165`（條件只在讀取時計算）；`src/item_info/item.rs:650`（`unk_pre_gimmick_visual: u32 => equip_type_info.0 != 0 || item_type == 74`）。
- **問題與根因：** 讀取端依 `equip_type_info` / `item_type` 決定這 4 B 在不在；寫回端只看 dict 裡這個欄位是不是 `None`。使用者把某個 item 的 `equip_type_info` 從 0 改成非 0（或把 `item_type` 改成 74），卻沒有同時把 `unk_pre_gimmick_visual` 從 `None` 改成整數，寫出的 row 少 4 B，重新讀取時從這裡開始錯位。反方向同理。不報錯。
- **可重現的情境：** 取一個 `equip_type_info == 0` 且 `item_type != 74` 的 item，把 `equip_type_info` 改成 1，`serialize_iteminfo` 後再讀回。
- **嚴重度：** low。
- **確信度：** confirmed（程式碼）／plausible（後果）。
- **來源：** 既有問題（`0694dfb`）。
- **建議方向：** `write_from_py_dict` 寫到條件欄位時，用 dict 裡已寫過的欄位重算條件，不一致時回 `ValueError`。

### H-13 — `.pyi` 型別存根與實際 API 脫節

- **位置：** `python/crimson_rs/__init__.pyi`，對照 `src/python.rs:1891-1926` 與 `src/item_info/{item,structs}.rs`。
- **問題與根因：**
  1. **缺函式（6 個）：** `parse_iteminfo_lossy`、`parse_save_from_bytes`、`parse_save_from_file`、`write_save_with_nonce`、`parse_save_body_from_bytes`、`decode_save_body_blocks` 都有註冊，`.pyi` 沒有。
  2. **`ItemInfo` TypedDict（`:361-607`）：** 有 7 個實際不存在的欄位：`money_icon_path`（`:397`，1.10 已刪）、`prefab_data_list`（`:492`）、`gimmick_visual_prefab_data_list`（`:494`，1.13 已合併）、`new_icon_path`、`ammo_mid_block`、`icon_flag`、`icon_unk_zeros`（`:567-583`）。缺 9 個實際存在的欄位：`is_blocked_store_sell`、`is_preserved_on_extract`、`is_has_item_use_data_inventory_buff`、`is_preorder_item`、`is_equip_quick_slot_visible`、`unk_post_apply_drop_stat_type`、`unk_pre_max_endurance_a`、`unk_pre_gimmick_visual`、`merged_prefab_visual_list`。型別不符：`max_endurance`、`respawn_time_seconds` 標成 `int | None`，實際是必填整數；`item_name` / `item_desc` / `item_desc2` 標成 `LocalizableString`，實際是 `OptLocalizableString`（category 0 時只寫 1 B，`index` / `default` 的編輯會被丟掉，`types.rs:401-410`）。`.pyi:586-592` 自己有註明「partial view」，但 TypedDict 預設 `total=True`，照存根建 dict 會在寫回時 `KeyError`。
  3. **巢狀結構：** `ItemIconData`（`:30-36`）缺 `icon_path_alt`、`unk_flag`；`EnchantData`（`:103-108`）缺 `unk_post_equip_buffs`；`InspectData`（`:152-190`）缺 `unk_post_inspect_effect_a/_b`；`DockingChildData`（`:292-339`）缺 `unk_post_summon_tag_a/_b`；`UnitData`（`:217-224`）缺 `unk_post_icon_path`；沒有 `MergedPrefabVisualData`；`SubItem`（`:261-265`）只寫 14 是 None，實際是 14–18。
  4. **其他：** `TrackedItemInfo`（`:702-704`）缺 `error`、`error_span`；`SkillEntry`（`:1307-1326`）缺 `unk_post_key`（`python.rs:1372`、`:1429`）；`FieldRange.path` 的範例 `"enchant_data_list.0.level"`（`:683`，同 `python.rs:258`、`docs/api.md:342`）實際格式是 `"enchant_data_list[0].level"`（`binary/mod.rs:70-76` 的 `push_index`）；`PamtChunk.checksum` 寫「CRC32」（`:934`），實際是 Jenkins `hashlittle`（`paz.rs:522`）；`add_papgt_entry` 寫「recalculated checksum」（`:1108`），2.04+ 沒有 checksum，trailer 會過期。
  5. 套件裡沒有 `py.typed`，依 PEP 561，型別檢查器對安裝後的套件可能不採用這份存根（plausible，視工具設定）。
- **可重現的情境：** 依 `.pyi` 建一個 `ItemInfo` dict 交給 `serialize_iteminfo` → 第一個缺的欄位 `KeyError`；`mypy` 對 `crimson_rs.parse_iteminfo_lossy` 報「has no attribute」。
- **嚴重度：** low。
- **確信度：** confirmed。
- **來源：** 既有問題（`row_index` / `header` 參數已在 `95fa25f` 補上）。

### H-14 — 文件與程式碼脫節；`pack_mod` 章節沒有 2.04+ 警語

- **位置與問題：**
  1. `docs/api.md:31-45` 的 `Language` 表與 `python/crimson_rs/enums.py:20-36` 完全不同：文件有 `CHT=0x0008`、`GER=0x0010`、`FRA`、`SPA`、`POR`、`THA`、`IND`、`CHS`，實際是 `RUS=1<<3`、`TUR=1<<4`、`SPA_ES`、`SPA_MX`、`FRE`、`GER=1<<8`…，`Language.CHT` 不存在。照文件用數值會設錯語言旗標。
  2. `docs/api.md:77` 說 `crypto` 可以是 `.ICE` 或 `.AES`，`enums.py:13-17` 也提供這兩個值，但 `src/python.rs:844-854` 只接受 0 與 3，傳 1 / 2 會 `ValueError("unsupported crypto")`。ChaCha20 的路徑另有 A-04 的問題。
  3. `docs/api.md:446`「Each item is a dict with 105 fields」，實際 `ItemInfo` 有 124 個欄位（`item.rs:525-681`）。`:505`（`money_icon_path`）、`:508-509`（`prefab_data_list`、`gimmick_visual_prefab_data_list`）、`:609`（`usable_alert`，實際是 `usable_alert_type`）都是已不存在的名稱；巢狀結構的範例（`:684-692`、`:773-782`、`:906-931`、`:964-991`、`:1011-1021`）也缺 H-13 第 3 點列的欄位。`:267` 的「ItemInfo (pabgb)」標題下第一個條目是 `parse_skillinfo_from_bytes`。
  4. `docs/api.md:52-92` 的 `pack_mod` 章節寫「This is the main entrypoint for modders」，整節沒有 2.04+ 的警語（警語只在 `README.md:31` 與 `docs/archive-format.md:274`）。`pack_mod.py:107-109` 印出的「Done! To install, copy these into the game directory」在 trailer 警告之後無條件出現。依 CLAUDE.md「不得宣稱 2.04+ 的 `pack_mod` 輸出能被載入」，這兩處的語氣偏向過度承諾。
  5. `docs/archive-format.md:272` 見 H-07。
  6. 封存路徑範例停在 2.00：`src/python.rs:1032`、`.pyi:1130`、`.pyi:1401` 用 `gamedata/binary__/client/bin/iteminfo.pabgb`，2.01 起是 `binarystaticinfo__/bin/*.staticinfobody`（`docs/api.md:3-10` 已更新）。
  7. `README.md:95` 說 Python 綁定是用來處理 `iteminfo_decompressed.pabgb` 的，與 H-02 有關的「byte-perfect roundtrip」說法見該條。
- **可重現的情境：** `from crimson_rs import Language; Language.CHT` → `AttributeError`；`PackGroupBuilder(out, crypto=int(Crypto.AES))` → `ValueError`。
- **嚴重度：** low。
- **確信度：** confirmed。
- **來源：** 既有問題。

### H-15 — 沒有版本自檢，stale `.pyd` 在執行期無法偵測

- **位置：** `python/crimson_rs/__init__.py:1-2`（只做 `import *`）；`src/python.rs:1891-1926`（沒有 `__version__`、build stamp、paver 或 `PARSER_TARGET_*` 相關函式）；`Cargo.toml:3`（`version = "0.1.0"`，從未變動）、`pyproject.toml:8`（`dynamic = ["version"]`，取自 Cargo）；`python/crimson_rs/validate_game_dir.py`（不檢查 crimson-rs 版本，也不讀 `meta/0.paver`）。
- **問題與根因：**
  1. CLAUDE.md 把 stale `.pyd` 列為新 patch 時「先查這個」的頭號陷阱，給的檢查方法是看檔案 mtime。程式裡沒有任何可查的身分：`importlib.metadata.version("crimson-rs")` 永遠是 `0.1.0`，模組沒有 `__version__`、沒有 git hash，也沒有「這個 build 的目標 gamedata 版本」。
  2. C ABI 有 `crimson_parser_target_gamedata_major/_minor`、`crimson_parser_min_gamedata_patch` 與 paver 解析（見 `docs/c-abi-surface.md`），Python 完全沒有對應物，所以 Python 使用者無法實作 CLAUDE.md 要求的「major/minor 成對比較 + patch 下限」閘門，只能靠解析失敗來發現版本不符。
  3. 這次看到的實例：in-tree `python/crimson_rs/crimson_rs.pyd` 的 mtime 是 2026-11-27 11:11:17，`95fa25f` 的 commit 時間是 11:27:06。從 `95fa25f` 到 `e465942`，Python 可達的原始碼只有 `src/item_info/item.rs` 改了 2 行（`git diff --stat`），所以這份 `.pyd` 應該是新的，但**沒有任何辦法從 Python 端確認**。
- **可重現的情境：** 在一個舊 build 的 venv 裡 `import crimson_rs`，沒有任何 API 能告訴你它是哪個 commit、目標哪個 patch。
- **嚴重度：** low。
- **確信度：** confirmed。
- **來源：** 既有問題。
- **建議方向：** 用 `env!("CARGO_PKG_VERSION")` 加 build script 注入的 git hash 暴露 `__build__`；暴露 `parser_target_gamedata()` 與 paver 解析；`validate_game_dir` 先比對兩者。

### H-16 — 2.06 的 lossy／tracked 路徑不收 header，錨點變弱

- **位置：** `src/python.rs:100-120`（`scan_next_item_start`）、`:122-171`（`parse_iteminfo_lossy`）、`:173-244`（`parse_iteminfo_tracked`）；`README.md:211-223`。
- **問題與根因：**
  1. 2.06 之前的錨點要求 `[u32 len][可列印名稱][key]`，很難誤中；2.06 改成「`key != 0` 且 `< 2^24`、`row index < 100,000`、下一個 byte 為 0」（`:118`），9 個位元組裡只有很弱的條件。row 內任何「小 u32、小 u32、0」的組合都會被當成新 row 的起點，`skipped_bytes` 與 `recovered_at` 的意義因此變弱，也可能把垃圾誤解成成功的 item（每個 item 有很多 tag 檢查，所以機率不高，但沒有量化）。`:111-113` 的註解有提到「weaker anchor」。
  2. 2.06 的 header 本來就給出每一列的精確 offset 與 size，lossy 與 tracked 卻都不收 header，放棄了最可靠的錨點；而且兩者在 2.06 上的 `string_key` 一律是 `""`，`.pyi` 與 `docs/api.md` 都沒寫。
  3. 錨點要求 `is_blocked == 0`，所以 `is_blocked != 0` 的列永遠不會被當成恢復點（2.06 之前的版本也有這個條件，既有）。
  4. B 區塊報告已確認 `index < 100_000` 這個條件對 DLC（167..8342）成立，這條不重複。
- **可重現的情境：** 讓 2.06 body 的第 N 列在中段解析失敗（例如改壞一個 `SubItem` tag），`parse_iteminfo_lossy(body)` 的 `recovered_at` 很可能落在第 N 列內部而不是第 N+1 列的起點。
- **嚴重度：** low（這是診斷／救援工具）。
- **確信度：** confirmed（缺 header、錨點條件）／plausible（誤錨率沒有量化）。
- **來源：** 本次引入（錨點與 `string_key == ""`，`95fa25f`）／既有（`is_blocked` 條件與函式本身，`dc0b6a3`）。

### H-17 — Python 可達的 abort 與 `PanicException`

- **位置：** `src/python.rs:802-827`（`decompress_data(…, uncompressed_size: usize)`）→ `src/binary/paz.rs:35`（`lz4_flex::block::decompress(data, uncompressed_size)`）、`:40`（`Vec::with_capacity(uncompressed_size)`）。交叉引用：A-01（`parse_pamt_bytes` / `extract_file*` 的 trie 遞迴）、A-02（依 PAMT 宣告值配置）、F 報告（`parse_save_body_from_bytes` / `decode_save_body_blocks` 的 `toc_count` 配置）、H-05 第 3 點。
- **問題與根因：**
  1. PAMT 裡的 `uncompressed_size` 是 u32，`decompress_data` 卻直接收 Python 的 `usize`，上限是 2⁶⁴。超過 `isize::MAX` 時是 capacity overflow panic，PyO3 轉成 `pyo3_runtime.PanicException`；介於實體記憶體與 `isize::MAX` 之間（例如 `1 << 40`）時是真正的配置失敗，`handle_alloc_error` 直接終止直譯器。
  2. `PanicException` 繼承自 `BaseException` 而不是 `Exception`，一般的 `except Exception:` 接不到，對使用 lossy／救援流程的腳本來說是個陷阱。
  3. 上面交叉引用的 abort 在 Python 端一樣會終止整個直譯器（stack overflow 與配置失敗都不是 unwind）。
- **可重現的情境：** `crimson_rs.decompress_data(b"\x00", 3, 1 << 40)`。依程式路徑預期 zlib 分支先 `with_capacity(1 TiB)` → 配置失敗 → 行程終止。
- **嚴重度：** low（需要呼叫端傳入荒謬的值，或惡意封存）。
- **確信度：** plausible（沒有執行；`lz4_flex` 的配置行為依 A 報告同樣的假設）。
- **來源：** 既有問題（`8623be0`）。

### H-18 — Feature gate 與死碼

- **位置：** `.github/workflows/ci.yml:48-58`；`src/lib.rs:12-15`、`:66-67`、`:89-90`；`src/python_traits.rs:150-204`（`[u8; 22]`、`[u8; 9]`）；`src/item_info/structs.rs:309-317`（`GimmickVisualPrefabData`）、`:431-438`（`PrefabData`）。
- **問題與根因：**
  1. CI 編譯 `default`（`cargo check --lib`）與 `c_abi,python`（clippy），沒有編譯只開 `python` 的組合，而 `pyproject.toml:16` 的 wheel 正是只開 `python`。讀碼看起來它能編譯：`python.rs` 只用無條件公開的 `crate::save` 項目（`src/save/mod.rs:48-63`，F 報告也這樣判斷），`static_header` 沒有 feature gate。沒有實際編譯驗證。
  2. Python 綁定沒有任何測試目標（H-11）。`extension-module` 讓 PyO3 不連結 libpython，所以就算加 Rust 端的 `#[test]` 呼叫 Python API 也連結不起來，需要另外的 Python 測試流程。
  3. `[u8; 22]` / `[u8; 9]` 的 `ToPyValue` / `WritePyValue`（以及 `binary/arrays.rs:101-171` 的讀寫）只服務已移除的 1.05 欄位；`PrefabData`、`GimmickVisualPrefabData` 在 1.13 合併後已沒有任何欄位使用，卻仍在 `.pyi`（`:111-121`、`:239-247`）與 `docs/api.md`（`:871-892`）裡被當成現行結構說明。trait impl 與被 trait impl 建構的 struct 不會觸發 `dead_code`，所以嚴格 clippy 抓不到。
- **嚴重度：** nit。
- **確信度：** confirmed（CI 設定、死碼）／plausible（只開 `python` 能否編譯）。
- **來源：** 既有問題。

### H-19 — `serialize_skillinfo` 的靜默降版與 key 不一致

- **位置：** `src/python.rs:1497-1500`（`row_indices` 缺 key 或為 `None` → `None`）、`src/skill_info/mod.rs:189-210`（`row_indices` 為 `None` 時寫 pre-2.06 佈局；pre-2.06 的 header key 取 `index_order[i].key`，不看 `entries[i].key`）。
- **問題與根因：**
  1. parser 只追 2.06，但寫回端在 dict 缺 `row_indices` 時會靜默寫出 2.04–2.05.01 的佈局。`docs/api.md:304` 有寫這個行為，所以只算 nit；不過對「只支援最新版」的 parser 來說，降版應該要明確要求（例如 `layout="legacy"`），而不是靠少一個 key 觸發。
  2. pre-2.06 路徑用 `index_order[i].key` 寫 header、用 `entries[i].key` 寫 body，兩者不一致不報錯。2.06 路徑由 `from_legacy_layout` 從 body 取 key，不受影響。
- **嚴重度：** nit。
- **確信度：** confirmed（程式碼）。
- **來源：** 本次引入（`row_indices`，`95fa25f`）／既有（key 來源）。

---

## 跨區塊關注點對照

| 主題 | H 區塊的結論 |
|---|---|
| 不可信輸入配置上限 | Python 自己的配置點：`wr_papgt_from_dict` 的 `resize`（H-05）、`decompress_data` 的 `usize`（H-17）。其餘都經過 A／F 的解析器，見 A-01、A-02、F 報告。`serialize_skillinfo` 的 `Vec::with_capacity(entries_list.len())`（`python.rs:1485`、`:1489`）以 Python list 長度為上限，安全。 |
| Panic 面與 abort | dict 轉換全部走 `PyResult`，沒找到會 panic 的 `unwrap` / 索引（見下方「已排除」）。PyO3 會把 panic 轉成 `PanicException`（`BaseException` 子類別），但擋不住 stack overflow 與配置失敗（H-17）。 |
| Roundtrip 對稱性 | Python 寫回是另一份實作（H-11）；PAMT（H-03、H-04）、PAPGT（H-05）的 dict 往返都有不對稱；條件欄位（H-12）、定長陣列（H-10）會寫出讀不回來的 row。2.06 header 沒有寫回路徑（H-02）。 |
| 版本漂移明確失敗 | `parse_iteminfo_from_*` 帶 header 時會明確失敗（好）。`inspect_legacy_patches` 靜默全 `None`（H-01）、lossy／tracked 靜默 `string_key == ""`（H-16）、`serialize_skillinfo` 靜默降版（H-19）、Python 沒有版本閘門（H-15）。 |
| Feature gate 組合 | 只開 `python` 的組合 CI 不編譯；trait impl 形式的死碼 lint 看不到（H-18）。 |
| `docs/c-abi-surface.md` 一致性 | 不適用：H 區塊沒有 `extern "C"` 符號。Python 介面對應的文件是 `docs/api.md` 與 `.pyi`，它們的一致性問題見 H-13、H-14。 |

---

## 已排除（查證後沒有問題，或不構成 finding）

計畫書的「掃描時注意到的待查事項」**沒有任何一條標給 (H)**，所以沒有需要排除的待查事項。下面是審計重點裡查過、結論為「沒有問題」的部分：

1. **審計重點 1：dict → Rust 轉換在缺 key、型別錯誤時的行為。** 缺 key 一律回 `PyKeyError`（`python.rs:16-28` 的 `get` / `get_obj`、`python_traits.rs:23-26` 的 `get_field`、`py_binary_struct!` 的 `__pbs_write_py!` 也走 `get_field`）；型別錯誤由 `cast`（`TypeError`）或 `extract`（`TypeError` / `OverflowError`）回報；定長欄位多數有長度檢查（`python.rs:612-614`、`:881-883`、`:1398-1404`、`:446-452`、`:1565-1576`）。沒有找到會 panic 的路徑：`skill_entry_from_py` 的 `pad_bytes[0..3]` 在長度檢查之後（`:1399-1405`），`wr_papgt_from_dict` 的切片在 `resize` 之後（`:426-432`），`inspect_legacy_patches` 的 `abs_pos - r.start` 在二分搜尋命中之後（`:331-338`）。例外只有 H-10 的「不檢查長度但也不 panic」。
2. **`inspect_legacy_patches` 的二分搜尋前提。** 註解（`python.rs:318-319`）說 ranges 依 start 排序、連續、不重疊、涵蓋每個位元組。查過 `CArray`（`types.rs:172-210`，記錄 `__count__`）、`CFlaggedArray`（`:246-269`，記錄 `__tag__`）、`EnchantDataList`（`structs.rs:201-276`，記錄 tag、count、separator）、定長陣列（`arrays.rs:117-259`）都只記錄葉節點、依讀取順序推入，前提成立。`HeaderName` 不推入 range，不影響。問題只在 H-01 的索引鍵。
3. **審計重點 3：不得繞過 PAPGT trailer。** Python 端對 trailer 只有「原樣讀出、原樣寫回、長度必須 64」（`python.rs:380-384`、`:446-453`），沒有任何重算或偽造的路徑；`pack_mod` 與 `create_pack` 都會印警告（`pack_mod.py:95-100`、`create_pack.py:126-131`）。不足之處（`validate_game_dir` 沒有警告、文件語氣）列在 H-07、H-14。
4. **`write_save_with_nonce` 設 `hmac_ok: true`（`python.rs:1585-1589`）。** `Save::write_with_nonce` 一律重算 HMAC 與 payload / uncompressed size（`src/save/io.rs:152-168`），`hmac_ok` 在寫回路徑不被讀取，所以這個寫死值沒有效果。HMAC 驗證失敗仍允許載入的語意屬 F 區塊，見 F 報告。
5. **`extract_file` / `extract_file_from_paz` 的路徑處理。** 只讀檔，`group_name` / `paz_path` 由呼叫端提供；穿越只會讀到呼叫端本來就能讀的檔案，列在 H-09 第 3 點作為備註，不單獨成條。
6. **`create_pack` / `pack_mod` 的 `rel.parent` 轉換。** `str(rel.parent).replace("\\", "/")` 與 `"." → ""` 的處理正確，`rglob` 得到的相對路徑不會含 `..`，所以 mod 資料夾內容本身不會造成 VFS 名稱穿越。
