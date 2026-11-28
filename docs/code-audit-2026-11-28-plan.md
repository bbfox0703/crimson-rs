# Code audit plan — 2026-11-28

把 crimson-rs 拆成 **9 個可獨立審計的功能區塊**，每塊列出範圍、職責、對外介面、依賴、審計重點與驗證方式。本文件只是**計畫**：掃描時沒有修改任何程式碼，下文「掃描時注意到的待查事項」也都尚未驗證，不是 finding。

- **基準：** `dev` @ `e465942`，遊戲版本 2.06（paver `2/6/0`）。
- **規模：** `src/` 116 個 `.rs` 檔、約 69.7k 行（含 in-file `#[cfg(test)]`，約四成是測試）；`python/crimson_rs/` 約 1.8k 行；`scripts/` 48 個腳本、約 8.5k 行；`data/` 為各版本的 key snapshot。
- **行數** 以 `Grep ^ --count` 計算，含註解與測試，只用來估算工作量。

---

## 區塊總覽

| # | 區塊 | 主要路徑 | 約略行數 | 風險 | 依賴 |
|---|---|---|---:|---|---|
| A | 封存容器與加密 | `src/binary/{primitives,types,arrays,mod,pamt,papgt,paz,trie,paloc,paver}.rs`, `src/crypto/`, `src/c_abi/{paz,paloc,paver,checksum}.rs` | 7.3k | 高 | — |
| B | Static-info 表頭／列頭佈局層 | `src/binary/{static_header,row_head,gamedata_layout}.rs`, `src/skill_info/pabgh.rs`, `src/string_info/pabgh.rs`, `src/c_abi/mod.rs:77-808`, `scripts/gamedata_layout.py` | 1.9k | 高 | A |
| C | 核心大型解析器：ItemInfo / Skill / StringInfo | `src/item_info/`, `src/skill_info/{mod,buff_data,post_buff}.rs`, `src/string_info/mod.rs`, `src/c_abi/{iteminfo,skill_info,string_info}.rs` | 7.6k | 高 | A, B |
| D | 一般 gamedata key-resolver 橋接（27 張表 + 策展表） | `src/<table>_info/mod.rs` ×27, 對應 `src/c_abi/<table>_info.rs`, `src/c_abi/{main_quest_chapter,side_quest_faction,live_titles}.rs` | 19k（含測試） | 中 | A, B |
| E | 染色／Prefab 子系統 | `src/{dye_color_group_info,part_prefab_dye_slot_info,part_prefab_dye_texture_pallete_info}/`, `src/c_abi/{dye_color_group_info,part_prefab_dye_slot_info,part_prefab_dye_texture_pallete_info,item_part_prefab}.rs` | 7.4k | 中 | A, B, C |
| F | 存檔格式核心 | `src/save/`（`header`, `crypto`, `io`, `body/{decoder,encoder,object,schema,toc}`） | 3.3k | 高 | A（crypto） |
| G | 存檔編輯 C ABI | `src/c_abi/mod.rs`（`77-808` 以外的正式碼）, `src/c_abi/{element_template,all_items,positions}.rs` | 7.5k + 4.5k 測試 | 高 | F |
| H | Python 綁定與 mod 打包 | `src/python.rs`, `src/python_traits.rs`, `python/crimson_rs/*`, A 裡只被 `python` feature 用到的 builder | 4.1k | 中 | A, C |
| I | 建置、CI、測試基礎設施、腳本與資料 | `Cargo.toml`, `pyproject.toml`, `.cargo/config.toml`, `.github/workflows/ci.yml`, `src/lib.rs`（tests）, `src/{test_fixtures,repo_doc_budget}.rs`, `src/c_abi/character_info.rs:766-8127`（tests）, `scripts/`, `data/`, `references/`, `tools/` | 8.5k 腳本 + 9k 測試 | 低到中 | 全部 |

依賴方向：`A → B → {C, D, E}`、`A → F → G`、`{A, C} → H`、I 橫跨全部。審計彼此可以平行進行，只是 B 的結論會影響 C、D、E 的判讀，F 的結論會影響 G。

---

## A. 封存容器與加密

**職責：** 讀寫遊戲封存。PAPGT（全域描述）→ PAMT（每組索引）→ PAZ（實際資料塊，ChaCha20 加密，LZ4/zlib 壓縮），加上 trie 名稱樹、PALOC 本地化字串表、`meta/0.paver` 版本戳，以及 Jenkins `hashlittle2` checksum（同時是 PAMT/PAPGT 完整性檢查和 PALOC key 的雜湊）。

**範圍：**
- `src/binary/`：`primitives.rs` 157, `types.rs` 464, `arrays.rs` 259, `mod.rs` 323, `pamt.rs` 477, `papgt.rs` 342, `paz.rs` 1261, `trie.rs` 539, `paloc.rs` 298, `paver.rs` 320
- `src/crypto/`：`chacha20.rs` 54, `checksum.rs` 120
- C ABI 薄包裝：`src/c_abi/paz.rs` 1368（含 `crimson_paz_list_npc_portraits`）, `paloc.rs` 607, `paver.rs` 480, `checksum.rs` 183

**對外介面：** `BinaryRead` / `BinaryWrite` trait、`CString` 等序列化型別、`paz::extract_file`、`crimson_paz_*`、`crimson_paloc_*`、`crimson_paver_*`、`crimson_checksum_*`。

**審計重點：**
1. **不可信長度欄位**：`u32 count` / `u32 len` 直接拿去 `Vec::with_capacity` 或切片，有沒有上限與越界檢查（壞檔或惡意 mod 檔不能造成 OOM 或 panic）。
2. **解壓縮**：LZ4 / zlib 的輸出大小以 PAMT 宣告值為準時，宣告值不實的處理方式。
3. **Roundtrip**：`read → write` 是否位元組一致（專案規則），尤其是 PAPGT 的 signature-like trailer、paver 的 10 / 14 位元組 / DLC 清單三種形狀。
4. **ChaCha20 / checksum**：key/nonce 推導與 `hashlittle2` 變體的邊界（長度 0、非 12 倍數的尾段）。
5. **`paver` 版本判定**：`PARSER_TARGET_GAMEDATA_MAJOR/_MINOR` 與 `PARSER_MIN_GAMEDATA_PATCH` 必須成對比較（CLAUDE.md 的不變式）。

**驗證：** `cargo test --lib --features c_abi -- --test-threads=4`（`binary::`、`crypto::`、`c_abi::paz`、`c_abi::paloc` 相關測試）；離線測試用構造的截斷／超長輸入。

---

## B. Static-info 表頭／列頭佈局層

**職責：** 所有 gamedata 表共用的「版本漂移吸收層」。2.01 改目錄與副檔名（`binary__/client/bin/*.pabgb` → `binarystaticinfo__/bin/*.staticinfobody`），2.04 把列頭改成 `[len][name][key]`，2.06 把名稱移進 `.staticinfoheader` 並在 body 列首加上 `[key][u32 row index]`。`static_header::legacy_pair` 在記憶體裡重建 2.06 之前的佈局，讓其他表的解析器不用改。

**範圍：** `src/binary/static_header.rs` 309, `row_head.rs` 67, `gamedata_layout.rs` 205（test-only）, `src/skill_info/pabgh.rs` 149（多張表共用的 `parse_pabgh`）, `src/string_info/pabgh.rs` 202, `src/c_abi/mod.rs:77-808`（2.06 row names、`impl_name_only_bridge!` 巨集）, `scripts/gamedata_layout.py` 184。

**審計重點：**
1. `count` 寬度（1/2/4）與 `key` 寬度（1/2/4/8/12）的每種組合都有測到。
2. 2.06 header 的 `offset + size` 鏈是否驗證「連續、無重疊、最後一列剛好到 body 結尾」，不合時是回錯還是靜默截斷。
3. `legacy_pair` 重建出的位元組與真正的 2.05.01 佈局是否一致（可以拿 `gamedata-bin/` 裡保留的舊版 binary 比對）。
4. `*_load_from_bytes_with_header` 和舊的 body-only 入口：漏傳 header 時的錯誤路徑。
5. Rust 與 Python 兩份 `gamedata_layout` 查表是否同步（newest-first、fallback 順序）。
6. DLC 表的 row index 不是自身位置（`scripts/CLAUDE.md` 2.06 節），確認沒有任何程式碼假設它等於位置。

**驗證：** `cargo test --lib static_header row_head`；把 `data/gamedata-keys-2.06/*.txt` 與實際解析出的 key 集合比對。

---

## C. 核心大型解析器：ItemInfo / Skill / StringInfo

**職責：** 三張 schema 最複雜、只追最新版（2.06）的表。ItemInfo 必須在 7,068 個 base 與 8,218 個 `dlc_01` 物品上做到 byte-perfect；Skill 帶 buff data 與 post-buff 圖；StringInfo 直接讀 2.06 header。

**範圍：**
- `src/item_info/`：`item.rs` 765, `structs.rs` 973, `keys.rs` 111
- `src/skill_info/`：`mod.rs` 645, `buff_data.rs` 274（`TailSizeCache`）, `post_buff.rs` 422
- `src/string_info/mod.rs` 589
- C ABI：`src/c_abi/iteminfo.rs` 2708（正式碼約 1.3k：flags bitmask、`CrimsonItemInfoSummary`、socket / gem 交叉檢查、dye slot 12-byte mask）, `skill_info.rs` 590, `string_info.rs` 531

**審計重點：**
1. **`TailSizeCache` 的多 GB 配置**：CLAUDE.md 記載預設執行緒數下會非決定性地觸發。要找出根因：快取是否跨表或跨測試共用、錯誤的 tail size 是否被拿去配置。這是目前已知最具體的穩定性問題。
2. `Variant` / `optional<T>` 遇到未知 tag 時是回錯，還是猜測後繼續往下讀。
3. ItemInfo 欄位在 DLC 表才會出現非空值的情況（2.04 的教訓）：測試是否同時涵蓋 base 與 `dlc_01`。
4. `..._lookup_slot_mask`（3 bytes）是 partial read；確認文件與 `..._lookup_slot_mask_full` / `..._lookup_dye_slot_bits` 一致，呼叫端拿不到被截掉的資訊時是否有明確說明。
5. `CrimsonItemInfoSummary` 等 `repr(C)` 結構的大小與對齊，是否有 `const` assert 防止 C# 端的 layout 漂移。

**驗證：** `cargo test --lib --features c_abi -- --test-threads=4`（需要 live install）；`cargo test --lib _probe_skill_entry_failures -- --ignored --nocapture`。

---

## D. 一般 gamedata key-resolver 橋接

**職責：** 把存檔裡的 `XxxKey (u32)` 解析成內部名稱，可能的話再透過 PALOC 解析成本地化顯示名稱（有的經過 hash hop，有的直接用 key 當 PALOC hi32）。Save Editor workstream 的 35 個 bridge 大多在這裡。

**範圍（27 張表，核心約 5.2k 行，C ABI 約 13.9k 行含 in-file 測試）：**
- **D1 任務進度：** `quest_info`, `quest_gauge_info`, `mission_info`, `stage_info`, `knowledge_info`, `sub_level_info`, `global_game_event_info`, `global_game_event_group_info`, `game_advice_info`, `game_advice_group_info`，以及策展表 `c_abi/main_quest_chapter.rs` 1397, `c_abi/side_quest_faction.rs` 952, `c_abi/live_titles.rs`（test-only）
- **D2 世界／陣營／角色：** `character_info`（`c_abi` 正式碼 1-765，含 portrait matching）, `faction_info`, `faction_node_info`, `faction_relation_group_info`, `faction_spawn_data_info`, `mercenary_info`, `gimmick_info`, `region_info`, `trigger_region_info`, `house_info`
- **D3 經濟／雜項：** `store_info`, `royal_supply_info`, `item_group_info`, `craft_tool_info`, `craft_tool_group_info`, `gameplay_variable_info`, `reserve_slot_info`

D1、D2、D3 可以分給不同人平行審。

**審計重點：**
1. **`impl_name_only_bridge!` 產生的函式**：一次審巨集（B 區塊）就涵蓋所有 name-only 表；這裡只需要確認哪些表是手寫的（例如 `faction_info` 因為要保留 group key 而展開）以及手寫版本與巨集版本的行為是否一致。
2. **Key 寬度擴展**：u16 key 擴成 u32 時高位必須為 0；`characterinfo` 的 cat byte（hi8）剝除規則。
3. **PALOC namespace（lo32）常數**：每個 bridge 的預設 namespace 是否有測試釘住；已知的「巧合碰撞」namespace 不能被當成預設值。
4. **DLC merge（`*_merge_dlc_from_bytes`）**：key 衝突時的優先順序、沒有 DLC 時的行為。
5. **策展表**：`curated_titles_match_live_install` 能偵測到改名；策展來源文件（`docs/ref-gamedata/*.md`）與程式碼內常數是否同步。

**驗證：** `cargo test --lib --features c_abi -- --test-threads=4`；`docs/save-editor-keys-reference.md` 的 1.06 回歸基準。

---

## E. 染色／Prefab 子系統

**職責：** 染色編輯器背後的資料。`dyecolorgroupinfo`（顏色群組、色票解鎖 KnowledgeKey、主題名稱）、`partprefabdyeslotinfo`（每列一個 `.pac` mesh，key 是小寫路徑的 Jenkins checksum）、`partprefabdyetexturepalleteinfo`，以及 `item_part_prefab`：從 item 到 character prefab，再到 `.pac` mesh，再到 dye slot 的 join。

**範圍：** 核心 `dye_color_group_info` 655, `part_prefab_dye_slot_info` 640, `part_prefab_dye_texture_pallete_info` 425；C ABI `dye_color_group_info.rs` 923, `part_prefab_dye_slot_info.rs` 1621, `part_prefab_dye_texture_pallete_info.rs` 733, `item_part_prefab.rs` 2396。

**審計重點：**
1. `item_part_prefab` 的 join 會解析 `.prefab` 場景檔（從 PAZ 讀出的不可信文字或二進位）：解析的健壯性、路徑大小寫正規化與 checksum 是否一致。
2. 1,703 / 1,703 列 key 都等於路徑 checksum 這個不變式：有沒有測試釘住它，以便下個 patch 打破時能偵測到。
3. Dye slot 編號（存檔的 `_dyeSlotNo`）與 12-byte mask 的位元對應，是否和 C 區塊的 `lookup_dye_slot_bits` 一致。
4. Combined handle 的生命週期：多個底層 handle 組合在一起時，free 的順序與重複 free。

**驗證：** `docs/dye-editor-scope.md` 的範例；live-install 測試。

---

## F. 存檔格式核心

**職責：** 讀寫 `.save`：header、加密與 HMAC、I/O、body（自帶 schema、TOC、object block 的 decode / encode，以及每個缺席的 dynamic array / object list 一個 `0x01` absence marker）。

**範圍：** `src/save/mod.rs` 65, `header.rs` 124, `crypto.rs` 144, `io.rs` 175, `body/mod.rs` 129, `body/decoder.rs` 1324, `body/encoder.rs` 603, `body/object.rs` 492, `body/schema.rs` 165, `body/toc.rs` 94。

**審計重點：**
1. **Encoder 的 relocation**：CLAUDE.md 記載過「no-op decode → encode roundtrip 永遠不會移動 block，所以抓不到 relocation bug」。需要確認有沒有「改長度後 encode，再 decode」的對照測試。
2. Absence marker 的讀寫對稱性（缺席、存在但為空、存在且非空三種狀態）。
3. Schema 驅動的 decoder 遇到未知 `FieldKind` / meta kind 時的行為。
4. HMAC 驗證失敗時是否仍允許載入（`crimson_save_get_hmac_ok` 表示允許），以及寫回時是否重算。
5. Header / TOC 的 offset 與 size 都是不可信輸入：越界、重疊、整數溢位。

**驗證：** 需要 `crimson-rs-private` 的 save fixtures（1.09、1.10、length-change repro）；沒有 fixtures 時測試會 skip，CI 也不會有，所以這塊的審計**必須在有 fixtures 的機器上跑**。

---

## G. 存檔編輯 C ABI

**職責：** C# 編輯器（CrimsonAtomtic）使用的 handle-based 存檔 API：載入與釋放、inventory / character ref 列舉、scalar / float3 / float4 / uint4 / inline-bytes 讀寫、改長度的編輯（list insert / remove / clone / transplant）、deferred redecode batch、mutation version、block JSON、跨 patch 的 name-keyed element template、全容器物品列舉、有座標的實體列舉。

**範圍：** `src/c_abi/mod.rs`（1-76 與 809-4770；`error` 常數、`inventory_item_flags`、`scalar_getter!`、JSON 序列化）, `element_template.rs` 1047（`CRET` v1 wire format）, `all_items.rs` 1434（64-byte `CrimsonItemRecord`）, `positions.rs` 937；測試 `mod.rs:4771-9345`。

**審計重點：**
1. **FFI 安全性**：每個 `extern "C"` 入口都有 `catch_unwind`；null pointer 檢查；`from_raw_parts` 的長度來源（`src/c_abi/` 裡 `unsafe {` / `from_raw_parts` 共約 2.1k 處，`mod.rs` 就佔 500）；「先查詢大小、再填 buffer」的兩段呼叫在 buffer 不足時的回傳值。
2. **快照一致性**：`mutation_version` 在每個改長度的路徑上都有遞增；deferred redecode 開啟期間，讀取 API 的行為與 `docs/save-deferred-redecode.md` 是否相符；abort 後狀態是否回復。
3. **Element template 跨 schema 重建**：欄位消失、新增、kind 或 size 改變三種情況（應回 `TEMPLATE_MISMATCH`，不得靜默重新解讀）；`meta_kind` 4 locator 一律建立為 present 的特例。
4. **Batch API**（`set_scalar_fields_batch`、`list_remove_elements_batch`）在中途失敗時是全有或全無，還是部分套用。
5. `repr(C)` 結構（`CrimsonItemRecord` 等）的大小、對齊與 `docs/c-abi-surface.md` 一致。

**驗證：** fixtures + `cargo test --lib --features c_abi -- --test-threads=4`；`docs/save-mutation-version.md`、`docs/save-deferred-redecode.md` 的契約逐條對照。

---

## H. Python 綁定與 mod 打包

**職責：** PyO3 facade（`python` feature）把 PAPGT / PAMT / PAZ、ItemInfo、Skill 等暴露成 dict 往返的 Python API；`python/crimson_rs/` 提供 `pack_mod`、`create_pack`、`validate_game_dir`、`enums` 與型別存根。

**範圍：** `src/python.rs` 1926, `src/python_traits.rs` 365, `python/crimson_rs/__init__.pyi` 1411, `create_pack.py` 141, `pack_mod.py` 109, `validate_game_dir.py` 86, `enums.py` 36；以及 A 區塊裡只被 `python` 使用的 builder（PAZ / PAMT / PAPGT builder、trie builder、skill writer，見 `src/lib.rs:1-15` 的說明）。

**審計重點：**
1. dict → Rust 的轉換在缺 key、型別錯誤時是丟 `PyKeyError` / `PyValueError`，還是 panic。
2. `.pyi` 型別存根與實際 `#[pyfunction]` 簽名是否一致（約 105 個 pyfunction / pyclass / fn）。
3. `pack_mod` 輸出在 2.04+ **不保證**能被遊戲載入（CLAUDE.md 規則）；文件與 docstring 是否沒有過度承諾。審計時不得嘗試繞過 PAPGT trailer。
4. 打包路徑（Python 腳本拼接遊戲目錄）的路徑穿越與覆寫既有檔案的風險。
5. `abi3-py312` 與 stale `.pyd` 陷阱：`validate_game_dir.py` 或 import 時有沒有版本自檢。

**驗證：** 在 3.12 venv 跑 `maturin develop --release`，再 smoke test；`cargo clippy --all-targets --lib --features c_abi,python -- -D warnings`（`PYO3_PYTHON` 要用絕對路徑）。

---

## I. 建置、CI、測試基礎設施、腳本與資料

**職責：** 讓前八塊可以被建置與驗證的一切。

**範圍：**
- 建置：`Cargo.toml`、`Cargo.lock`、`pyproject.toml`、`.cargo/config.toml`（靜態連結 MSVC CRT）、feature 組合（`default` / `c_abi` / `python`）。
- CI：`.github/workflows/ci.yml`（clippy 全 feature 加 `cargo check` bare default 加 `cargo test`）。
- 測試設施：`src/lib.rs` 的 tests（約 1.4k 行）、`src/test_fixtures.rs`、`src/repo_doc_budget.rs`（CLAUDE.md ≤ 40 KB 守門）、`src/c_abi/character_info.rs:766-8127`（約 7.3k 行，49 個測試函式，含 live 整合測試）。
- 腳本：`scripts/` 48 個（`export_for_ce.py` 860、`update_ce_table.py` 619、各版 `diff_*.py`、`worldmap_*`、`scripts/archive/`）、`setup-fork-guards.ps1`。
- 資料：`data/keys-*.txt`、`data/gamedata-keys-<ver>/`（1.08 到 2.06，每版 30 張表）、`references/store_info.hexpat`、`tools/git-crypt.exe`。

**審計重點：**
1. **Skip 與 pass 的區分**：沒有 live install 或 fixtures 時測試會「skip」，但在 Rust 裡 skip 通常是 `return`，會被算成 pass。CI 綠燈的實際涵蓋範圍需要列出來。
2. `CRIMSON_GAME_ROOT` / `CRIMSON_FIXTURES_DIR` 覆寫是否被所有測試一致採用（`src/lib.rs:116` 仍有寫死的 `GAME_DIR`）。
3. 測試放置：`character_info.rs` 的 7.3k 行測試是否其實是跨 bridge 的整合測試、應該搬到獨立模組（只記錄，不在本次審計搬動）。
4. `.cargo/config.toml` 的 CRT 靜態連結與 `--features c_abi` 的產物確實零 transitive DLL 依賴。
5. `setup-fork-guards.ps1` 的 pre-push hook 是否涵蓋所有指向 upstream 的寫法。
6. `tools/git-crypt.exe` 是 checked-in 的二進位檔：來源與用途（只為了舊 commit 的加密 blob）是否有文件說明。
7. 依賴：`Cargo.lock` 的 `cargo audit` 或 `cargo deny`（工具只能在本機執行；新增依賴需要先詢問）。

---

## 跨區塊關注點

每塊審計時都要套用：

| 主題 | 檢查方式 |
|---|---|
| 不可信輸入 | 所有長度、count、offset 都來自遊戲檔、存檔或 mod 檔；用 `grep -n "with_capacity\|vec!\[.*; *\w*len"` 找配置點，逐一確認上限。 |
| Panic 面 | `unwrap()` / `expect()` / 索引 `[]` / 算術溢位（release 模式會 wrap）；C ABI 入口的 `catch_unwind` 只能擋住 unwind，擋不住 `abort`。 |
| Roundtrip | 專案規則：讀寫必須位元組一致。no-op roundtrip 不能證明欄位邊界，也抓不到 relocation，需要搭配修改後的往返測試。 |
| 版本漂移 | 只追最新版的解析器（C）與相容層（B）要能**明確失敗**，不能悄悄地讀錯；pin 被觸發時先檢查列數、`KNOWN` pin 與值分布。 |
| Feature gate | 每個模組在 `default` / `c_abi` / `python` / `c_abi,python` 四種組合下都能編譯；`cfg_attr(allow(dead_code))` 不能掩蓋真正的 dead code。 |
| 文件一致性 | `docs/c-abi-surface.md` 列出的函式與實際 `extern "C"` 符號一一對應。 |

---

## 建議審計順序

1. **B → A**：B 最小但影響面最大，A 是所有東西的地基。
2. **F → G**：存檔路徑的 bug 會直接寫壞使用者的存檔，風險最高；需要 fixtures。
3. **C**：先處理 `TailSizeCache`。
4. **E、D**（D1、D2、D3 可平行）。
5. **H、I**。

每塊的產出：在 `docs/code-audit-2026-11-28-<區塊代號>.md` 列出 findings，每條包含檔案:行號、問題、可重現的輸入或情境、嚴重度（critical / high / medium / low / nit）、確信度（confirmed / plausible），並區分「本次引入」與「既有問題」。依 CLAUDE.md 的規則，root cause 要先用位元組或可控的重現驗證，才寫成 confirmed。

---

## 掃描時注意到的待查事項（未驗證）

這些只是掃描時順手記下的線索，**都還沒有驗證**，分配給對應區塊確認：

- **(C)** `TailSizeCache = HashMap<u8, usize>`（`src/skill_info/buff_data.rs:34`）：快取 key 只有一個 `u8`，如果不同表或不同格式共用同一個 cache，tail size 可能被錯誤沿用。這可能和多 GB 配置有關。
- **(I)** `src/lib.rs:117` 的註解寫「parser targets Crimson Desert 1.05」，與目前的 2.06 不符，疑似過時註解。
- **(I)** `src/lib.rs:116` 的 `GAME_DIR` 是寫死的路徑；需要確認這組測試是否也讀 `CRIMSON_GAME_ROOT`。
- **(G)** `src/c_abi/mod.rs` 有 9,345 行（正式碼約 4.8k、測試約 4.5k），是 repo 最大的單一檔案，可讀性與審計成本都高；只記錄，不在本次審計中拆分。
- **(D)** 部分 bridge 的模組文件還寫著 1.07 的列數（例如 `craft_tool_info` 的「17 rows in 1.07」），可能已經過時。

---

## 限制

- 本計畫與後續審計**不修改任何程式碼**；findings 只記錄，修正另開 PR。
- live install（`D:\SteamLibrary\...` 或 `CRIMSON_GAME_ROOT`）與 `crimson-rs-private` fixtures 不在 CI 上；B、C、D、E、F、G 的部分結論只能在維護者的機器上驗證。
- Live-game RE 只能用唯讀的 `ReadProcessMemory`，不得寫入遊戲行程。
