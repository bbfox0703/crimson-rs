# Code audit — F. 存檔格式核心（2026-11-28）

依 [`code-audit-2026-11-28-plan.md`](code-audit-2026-11-28-plan.md) 的 F 區塊做靜態審計。**沒有修改任何程式碼**，沒有執行 crimson-rs 的任何程式或測試，沒有解密或解析任何存檔，也沒有碰 PAPGT trailer（F 區塊不涉及 PAPGT）。

- **基準：** `dev` @ `e465942`。下文的行號都以這個 commit 為準。
- **範圍：** `src/save/{mod,header,crypto,io}.rs`、`src/save/body/{mod,decoder,encoder,object,schema,toc}.rs`。為了判斷錯誤會傳到哪裡、哪些測試在守這塊，也讀了直接呼叫端與測試：`src/c_abi/mod.rs:809-885`（load / free）、`:1392`（`get_hmac_ok`）、`:1680-1756`（改長度的 commit 路徑）、`:3024-3049`、`:4158-4326`（write、deferred batch commit）、`:4851-4864`（c_abi 測試找存檔的方式）、`:5853-5942`；`src/c_abi/element_template.rs:111,424`；`src/python.rs:1518-1660`（Python 的 save 入口）；`src/lib.rs:523-1433`（save 測試）；`Cargo.toml` / `Cargo.lock`（依賴版本、沒有 `panic = "abort"`）。
- **依賴原始碼：** 這次讀得到 `~/.cargo/registry`，所以 `lz4_flex 0.11.6`（`src/block/decompress_safe.rs:354-355`）、`chacha20 0.10.0`（`src/variants.rs:37-53`）、`cipher 0.5.1`（`src/stream.rs:185-188, 247-249`、`src/stream/wrapper.rs:78-91`）的相關語意都直接看過原始碼，不是推論。
- **方法：** 逐行閱讀，追每個不可信數值（header、schema、TOC、block 內的 count / offset）從讀入到配置、索引、遞迴的路徑；對照 `docs/c-abi-surface.md`、CLAUDE.md、計畫書與程式內文件。
- **沒做到的事：**
  1. 計畫書要求 F 區塊在有 `crimson-rs-private` fixtures 的機器上驗證。本回合限定靜態審查，所以**沒有跑任何測試**，也沒有拿 fixtures 或 live save 做位元組比對。下文凡是行為後果都標 plausible，「未能驗證」一節列出建議的驗證方式。
  2. `git blame` 在本 session 需要額外核准而沒有執行。來源歸屬改用 `git log -- src/save`：`src/save/` 最後一次修改是 `4518157`（2026-09-18，absence marker），早於 2.06 移植週期的起點 `95fa25f`（2026-11-27），所以 **F 區塊的 finding 全部是既有問題**。各條標的引入 commit 是依 commit 訊息與程式內容推定的，不是 blame 結果。

### 確信度與來源歸屬的定義

沿用 A、B 區塊報告的定義：

- **confirmed**：只用原始碼或文件文字就能確定的事實，例如「沒有這個檢查」、「沒有這組測試」、「文件和程式碼不一致」。
- **plausible**：行為上的後果是靠追程式路徑或依賴的已知語意推出來的，**沒有用位元組或實際重現驗證過**。依 CLAUDE.md 與計畫書，這類一律標 plausible，即使程式路徑本身是確定性的。
- **本次引入**：引入的 commit 在 `95fa25f`（2026-11-27）到 `e465942` 之間。F 區塊沒有這類。
- **既有問題**：引入時間早於 `95fa25f`。主要來源：`3188491`（2026-05-12，從 Python 移植 header / crypto / io）、`ecf1c10`（2026-05-12，schema + TOC）、`735b678` / `61dd9cf` / `1aad9d7`（2026-05-12，field decoder、寬鬆走訪、trailing_pad）、`43242f0` / `ab815e6`（2026-05-14，encoder 與 payload_offset 正規化）、`f1513b8`（2026-06-05，list leading-pad 擴到 4）、`e6d000c` / `8b06ed5`（2026-06-09，trailing_pad relocation）、`4518157`（2026-09-18，absence marker 走訪）。

---

## 摘要

| # | 標題 | 嚴重度 | 確信度 | 來源 |
|---|---|---|---|---|
| F-01 | Encoder 不拒絕 decoder 沒解釋完的 block（undecoded 位元組、`Unknown` 欄位、placeholder 元素、非 inline child）；任何改長度的編輯都會重編**整個** body，靜默丟失或改寫其他 block 的位元組 | high | confirmed（缺檢查）／plausible（後果） | 既有問題 |
| F-02 | TOC `toc_count` 直接 `Vec::with_capacity`：`0xFFFFFFFF` 要約 128 GiB，配置失敗直接 abort | medium | plausible | 既有問題 |
| F-03 | Locator / object list 解碼無深度上限、無循環偵測：可 stack overflow（abort），而且多重回溯造成指數時間 | medium | plausible | 既有問題 |
| F-04 | 存檔核心的關鍵測試幾乎只讀 `%LOCALAPPDATA%` 的 live save，不讀 fixtures；`src/save/` 沒有離線合成測試；full roundtrip 只比長度與冪等，不比原始位元組；沒有「區塊內改長度後用獨立不變式檢查 offset」的測試 | medium | confirmed | 既有問題 |
| F-05 | `uncomp_size` 未驗證就交給 `lz4_flex`，safe-decode 會先 `vec![0; uncomp_size]`（最多 4 GiB，清零） | low | plausible | 既有問題 |
| F-06 | ChaCha20 用 nonce 前 4 位元組當 32 位元計數器，計數器跨過 2³² 時 `apply_keystream` panic；遊戲端的進位語意不明 | low | plausible | 既有問題 |
| F-07 | 構造輸入可觸發的 panic：marker 走訪呼叫 `try_decode_object_list_at` 前沒檢查 18 位元組；`marker_run_plus_zeros` 的邊界檢查差 1 | low | plausible | 既有問題 |
| F-08 | `trailing_pad` 內的 stale offset 只在「整個 block 搬家、內部不變長」時才會被 relocate；同一 block 內改長度時不會修正 | low | plausible | 既有問題 |
| F-09 | 版本漂移不會明確失敗：未知 `meta_kind`、mask byte count 超界、未知 class、TOC 截短、list 部分解碼、count=0 的 list 偏移猜測，都是「猜了繼續讀」 | low | confirmed（程式碼）／plausible（後果） | 既有問題 |
| F-10 | HMAC 驗證失敗仍可載入，寫回時一律重算 HMAC 而不看 `hmac_ok`：損壞或被竄改的存檔會被重新封成「有效」 | low | confirmed（程式碼）／plausible（後果） | 既有問題 |
| F-11 | Header / TOC 的 offset 與 size 只做到「不越界」，沒驗證順序、重疊、間隙、`stream_size`；encoder 假設 block 連續且依 TOC 順序，payload 之後與最後一個 block 之後的多餘位元組在寫回時被丟掉 | low | plausible | 既有問題 |
| F-12 | `scalar_from_bytes` 是公開 API 卻不驗證 `data.len() == size`（size 1 + 空切片 panic、長度不符靜默得 0）；`ScalarValue::Bytes` 長度不符只有 `debug_assert` | nit | confirmed（程式碼）／plausible（後果） | 既有問題 |
| F-13 | 文件與程式碼脫節：`stream_size` 定義、「no-op roundtrip 位元組一致」的三種互相矛盾的說法、不存在的 `docs/save-body-format.md`、「LZ4 HC」註解、`TooSmall` 說明、`c-abi-surface.md` 沒列存檔核心入口 | nit | confirmed | 既有問題 |
| F-14 | Dead code 被 `allow(dead_code)` 遮住：`scalar_as_i128` 在所有 feature 組合下都沒人用；encoder / object / `Body::write` 上「等 Phase B.2」的 allow 已過時 | nit | confirmed | 既有問題 |

沒有 critical。唯一的 high 是 F-01：它不需要惡意輸入，只要下一個 patch 讓任何一個 block 掉回舊走訪，使用者對**另一個** block 做一次改長度的編輯，就會把那個 block 的位元組靜默寫壞，而目前能抓到這件事的測試只在有 live save 的機器上跑（F-04）。F-02、F-03 和 A-01、A-02 同類：構造的存檔可以讓 C# 宿主行程直接 abort，`catch_unwind` 救不回來。存檔的加密金鑰寫在原始碼裡（`crypto.rs:15-29`），HMAC 驗證失敗也照樣解析（F-10），所以「必須先能產生合法存檔」不構成門檻；隨機的位元翻轉（磁碟損壞）同樣會走到這些路徑。

---

## Findings

### F-01 — Encoder 不拒絕 decoder 沒解釋完的 block，改長度的編輯會靜默寫壞其他 block

- **位置：**
  - Encoder：`src/save/body/encoder.rs:142-245`（`encode_body`：只檢查 `blocks.len() == toc.entries.len()`）、`:295-309`（`FieldKind::Unknown` 什麼都不寫）、`:375-461`（`inline_child == false` 時不重寫 child，卻把 `payload_offset` 改成 `wrapper_end`，`:449-450`）、`:463-480`（placeholder 元素照樣編碼）。模組文件自己寫了限制（`:38-44`：「Blocks with `undecoded_ranges` are NOT round-trippable」、「Non-inline locators … NOT supported」），但程式沒有任何檢查。
  - Decoder 會產生這些狀態：`src/save/body/decoder.rs:103-143`（舊走訪：`undecoded` 只有在剛好 1 段且 ≤ 16 B 時才收進 `trailing_pad`，其餘留在 `undecoded_ranges`）、`:226-293`（解不下去就 `break`，其後欄位維持 `Unknown`）、`:1122-1139`（list 元素解到一半就停，`count != elements.len()`）、`:1205-1242`（child 解不出來時回傳只有 wrapper 的 placeholder）、`:791-794`（inline 失敗就改用 `child_payload_offset` 的非 inline child）。
  - 觸發點：`src/c_abi/mod.rs:1707-1727`（`apply_length_changing_mutation`）與 `:4303-4321`（`crimson_save_end_deferred_redecode`）都把**全部** `h.blocks` 交給 `Body::write`，成功後直接取代 `h.save.body`；`crimson_save_write_to_file`（`:4167-4199`）寫出的就是這份 body。
- **問題與根因：**
  1. 舊走訪留下的 `undecoded_ranges`（> 16 B，或多於一段）在 encode 時直接消失：那些欄位是 `Unknown`，`encode_field` 不輸出任何位元組，`trailing_pad` 又是空的。block 變短，TOC offset 照新長度重算，所以結果在結構上仍可解析，不會有任何錯誤。
  2. Placeholder 元素（`mask_bytes` 空、`fields` 空）重編時：wrapper 寫 `mbc = 0`、沒有 mask（原本 1..16 B），再由 `encode_inline_payload` 憑空多寫 8 B（`reserved_u32 = 0` + trailing size 4）。原始只佔 `17 + mbc` B 的元素變成 25 B，內容也不同。
  3. 非 inline child（decoder 用 `child_payload_offset` 指向別處成功解出）：encoder 不重寫 child payload（`:457-459`），卻把 wrapper 的 `payload_offset` 改成 `wrapper_end`（`:449`），指向下一個欄位。
  4. 舊走訪的 list 部分解碼時，`header_bytes` 裡的 count 仍是原值，後面沒解到的元素位元組被下一個欄位的解碼器當成自己的內容「解出來」。位元組大致保得住，但值是錯的；之後若對這些欄位做編輯，寫回的是錯位的資料。
  5. Commit 路徑在 re-decode 之後也沒有做任何自我檢查（例如「新 blocks 再編一次要等於 `new_body`」、「沒有新的 undecoded / placeholder」、「blocks 數不變」），所以第一次出錯就直接成為 handle 的狀態。
  6. 參照：`src/c_abi/element_template.rs:111` 對 template 已經做了正確的拒絕（`!block.trailing_pad.is_empty() || !block.undecoded_ranges.is_empty()` 加深度上限），但 body 層的 encoder 沒有同樣的檢查。
- **可重現的情境：**
  - 漂移情境（非惡意）：下一個 patch 在某個 class（例如 `FactionSaveData`）加入一種 `decode_dynamic_array` 認不得的 header 形狀，使那個 block 的 marker 走訪回傳 `None`、舊走訪留下 > 16 B 的 undecoded。使用者在 C# 編輯器裡對**另一個** block 的 list 做 `crimson_save_list_insert_element`：呼叫回 `OK`，`h.save.body` 少了那些位元組，`crimson_save_write_to_file` 照樣寫出，遊戲讀檔時那個 block 錯位。
  - 構造情境：一個 list 元素的 child payload 讓兩種走訪都失敗，且 trailing size probe 也找不到 → placeholder；對同一個 save 做任何改長度編輯 → 該元素長度從 `17+mbc` 變 25，block 長度改變。
- **嚴重度：** high。後果是靜默寫壞使用者存檔，而觸發條件就是「版本漂移」這個本專案每個 patch 都會遇到的情境；`test_save_body_decode_all_blocks`（`src/lib.rs:794-802`）會在漂移時失敗，但它只在有 live save 的機器上跑（F-04），CI 與只有 fixtures 的機器都看不到。
- **確信度：** confirmed（encoder 與 commit 路徑沒有這些檢查，從原始碼可確定）／plausible（漂移時確實會發生、以及 placeholder 的實際位元組差異，沒有用位元組驗證）。
- **來源：** 既有問題（`43242f0` encoder；`61dd9cf` 寬鬆走訪；`1ba3051` commit 路徑）。
- **建議方向（不在本次修改）：** `encode_body` 在遇到 `undecoded_ranges` 非空、present 欄位為 `Unknown`、placeholder 元素、`inline_child == false` 的 locator、或 `count != elements.len()` 的 list 時回錯；或至少在 commit 路徑只允許「被編輯的 block 之外全部是 marker 走訪解出」的 body，並在 commit 後驗證 `encode(new_blocks) == new_body`。

### F-02 — TOC 的 `toc_count` 直接驅動 `Vec::with_capacity`，可 abort

- **位置：** `src/save/body/toc.rs:59, 63`。
- **問題與根因：** `toc_count` 是 body 裡的 u32，未與剩餘位元組比較就 `Vec::with_capacity(toc_count as usize)`。`TocEntry` 是 6 個 u32 加一個 `usize`，32 B，所以 `0xFFFFFFFF` 要約 128 GiB。這沒超過 `isize::MAX`，不會走可攔截的「capacity overflow」panic，而是真的去配置；Windows 不 overcommit，配置失敗時 `std` 預設的 `handle_alloc_error` **終止行程**。`crimson_save_load_from_file` 的 `catch_unwind`（`src/c_abi/mod.rs:830`）擋不住，Python 的 `parse_save_body_from_bytes` / `decode_save_body_blocks`（`src/python.rs:1609-1640`）一樣會讓直譯器結束。迴圈本身（`:64-69`）有長度檢查，只是配置在檢查之前。
- **可重現的情境：** 取任一存檔的解壓 body，把 `schema_end + 4` 的 u32 改成 `FF FF FF FF`，LZ4 壓縮後用 `derive_key(2)` 重新加密（或乾脆不重算 HMAC，見 F-10），交給 `crimson_save_load_from_file`。預期：`Toc::parse` 在第 63 行配置失敗，行程印出 `memory allocation of … bytes failed` 後終止。
- **嚴重度：** medium（需要構造或損壞的檔案，但結果是無法攔截的行程終止）。
- **確信度：** plausible（程式路徑確定，沒有實際執行）。
- **來源：** 既有問題（`ecf1c10`）。
- **建議方向：** 先算 `max_entries = (raw.len() - toc_start) / 20`，以 `min(toc_count, max_entries)` 配置；或不預先配置。

### F-03 — Locator / list 解碼無深度上限、無循環偵測

- **位置：** `src/save/body/decoder.rs:709-837`（`decode_inline_object_locator`，`:791-794` 在 inline 失敗時改用檔案裡的 `child_payload_offset`，tail 放寬到 `raw.len()`）、`:848-1014`（`decode_inline_object_payload`：先 `walk_fields_with_markers`，失敗再跑舊走訪，兩者都會再遞迴進 locator / list）、`:326-431`（marker 走訪，`:394-403`、`:404-423` 遞迴）、`:1016-1062`（`decode_object_list` 對 5 個 leading-pad 位置各試一次完整解碼）。
- **問題與根因：**
  1. **循環：** 非 inline fallback 用的 `child_payload_offset` 是檔案裡的任意 u32，沒有要求它在目前位置之後，也沒有「進行中」標記。payload X 裡的 locator 若把 `child_payload_offset` 寫成 X，而 inline 嘗試失敗，就會 `X → X → X …` 無限遞迴。
  2. **深鏈：** 即使沒有循環，inline 巢狀每層只要約 22 B（wrapper 17+mbc、reserved 4），1 MB 的 body 就能構成數萬層；每層要經過三個函式的 stack frame（`walk_fields_with_markers` 的 `DecodedField` 區域變數不小），C# 宿主執行緒預設 1 MB stack 很快就溢位。Rust 的 stack overflow 直接終止行程，`catch_unwind` 擋不住。
  3. **指數時間：** 每層失敗時會重試：marker 走訪失敗 → 舊走訪再解一次同一個 child（×2）；inline 失敗 → 改用 offset 再試（×2）；list 有 5 個 leading-pad 位置（×5）。最內層刻意失敗時，總工作量隨深度指數成長，約 30–40 層就足以讓載入卡住。另外非 inline fallback 的 trailing-size probe（`:988-999`）以 `raw.len()` 為 tail，每次失敗都線性掃到 body 結尾。
- **可重現的情境：** schema 定義型別 T，只有一個 `meta_kind = 4` 欄位；某 block 的 payload 在 X：`reserved` 4 B，接著在 X+4 的 wrapper 寫 `mbc = 1`、mask `01`、child type = T、`child_payload_offset = X`，並讓 `wrapper_end` 處的第一個 u16 是 0（inline 解碼的 `mbc` 不合法而失敗）。載入時預期無限遞迴 → stack overflow → 行程終止。
- **嚴重度：** medium（構造檔案即可讓宿主行程終止或卡死；真實存檔的巢狀深度很淺）。
- **確信度：** plausible。
- **來源：** 既有問題（`735b678` 遞迴解碼；`61dd9cf` 非 inline fallback；`4518157` 加上 marker 走訪後重試次數翻倍）。
- **建議方向：** 傳遞深度參數並設上限（`element_template.rs` 已有 `MAX_DEPTH` 的先例）；非 inline fallback 要求 offset 不在祖先鏈上，或乾脆只接受 inline（遊戲寫出的存檔都是 inline，見 `object.rs:152-156`）；把 trailing-size probe 限制在合理範圍。

### F-04 — 存檔核心的測試涵蓋：只讀 live save、沒有離線測試、roundtrip 斷言偏弱

- **位置：**
  - `src/save/` 只有 14 個 `#[test]`：`crypto.rs`（6 個，金鑰形狀、ChaCha20 自反、HMAC）與 `body/object.rs`（8 個，`scalar_from_bytes`）。schema、TOC、decoder、encoder 沒有任何離線合成測試。
  - `src/lib.rs:530-595`（`try_find_save`）與 `:987-1021`（`collect_all_saves`）只看 `%LOCALAPPDATA%\Pearl Abyss\CD\save`。`test_save_parse`、`test_save_write_parse_roundtrip`、`test_save_body_parse`、`test_save_body_decode_all_blocks`、`test_save_body_block_roundtrip_per_block`、`test_save_body_full_roundtrip`（`:597-1288`）都用它們；`src/c_abi/mod.rs:4851-4864` 的 `find_save` 只找 `slot107`，所有改長度的 c_abi 測試都依賴它。
  - 用到 `crimson-rs-private` fixtures 的只有：`test_save_body_absent_markers_every_live_save`（`:842-857`，live + fixtures）、`test_faction_revive_quest_no_trailer_relocates`（`:1300`）、`test_faction_node_offset_relocates_through_trailing_pad`（`:1390`）。
  - `test_save_body_block_roundtrip_per_block` 只比長度（`:1101-1105`）；`test_save_body_full_roundtrip` 只比長度與「第二次編碼 == 第一次」（`:1244-1264`），**不比原始位元組**。
- **問題與根因：**
  1. 計畫書與 CLAUDE.md 都說 F 區塊要在「有 fixtures 的機器」上驗證，但核心的 decode / roundtrip 測試根本不讀 fixtures。只有 fixtures、沒裝遊戲（或沒有 slot107）的機器上，這些測試以 `return` 結束而算成 pass；CI 上則全部如此。live save 也會隨遊玩改變，結果不可重現。
  2. Encoder 文件把「round-trip test would catch them」當成非 inline locator 的安全網（`encoder.rs:42-44`），但 full roundtrip 測試已不比原始位元組，非 inline locator 的 `payload_offset` 被改寫（F-01 第 3 點）只要長度不變、第二次編碼穩定，就不會被抓到。真正斷言「no-op 位元組一致」的只有 c_abi 的 `c_abi_list_clone_then_remove_roundtrip`（`src/c_abi/mod.rs:5936-5939`，需要 slot107）和兩個 faction 測試（只看一個 block）。
  3. **審計重點 1 的答案：** 「改長度後 encode，再 decode」的測試存在（`c_abi_list_clone_then_remove_roundtrip`、`c_abi_insert_empty_then_remove_roundtrip` 等），但它們用 decoder 檢查 encoder，而 decoder 先試 inline、忽略 `payload_offset`（`decoder.rs:774-794`），所以抓不到 relocation 錯誤，這正是 CLAUDE.md 記載的教訓。目前沒有任何測試在改長度之後，用**獨立的不變式**（例如「每個 inline locator / list 元素 wrapper 的 `payload_offset` 都等於自己的位置 + 4」）掃整個新 body；faction 的兩個 relocation 測試只測「整個 block 平移」，沒有測「block 內部變長」（F-08）。
- **可重現的情境：** 在只有 `D:\Github\crimson-rs-private` fixtures、`%LOCALAPPDATA%` 下沒有存檔的機器上執行 `cargo test --lib --features c_abi -- --test-threads=4`：上述 decode / roundtrip / c_abi 改長度測試全部印出 `skipping …` 並以 pass 結束。
- **嚴重度：** medium（F 是會直接寫壞使用者存檔的路徑，F-01 與 F-08 的回歸都要靠這些測試抓）。
- **確信度：** confirmed（測試內容與找檔方式從原始碼可確定）。
- **來源：** 既有問題（`3188491` 起的 save 測試；`ab815e6` 把 full roundtrip 降為冪等比較）。
- **建議方向：** 讓 `try_find_save` / `collect_all_saves` / c_abi `find_save` 也納入 `test_fixtures::save(...)`；為 schema / TOC / decoder / encoder 補幾個合成 body 的離線測試（三種 absence 狀態、非 inline locator、截短 TOC、undecoded block）；加一個「改長度後掃描整個 body 的 self-referential offset」的不變式檢查。

### F-05 — `uncomp_size` 未驗證就決定解壓配置大小

- **位置：** `src/save/io.rs:128-130`；`lz4_flex-0.11.6/src/block/decompress_safe.rs:354-355`（預設 feature `safe-decode`，`decompress` 先 `vec![0; min_uncompressed_size]`）。
- **問題與根因：** header 的 `uncomp_size` 是任意 u32，沒有和 `payload_size` 的合理比例（LZ4 最大壓縮比約 255:1）或任何絕對上限比較，就交給 `lz4_flex::block::decompress`，後者先配置並清零 `uncomp_size` 位元組。HMAC 失敗不會阻止這一步（`:126` 只記錄結果）。解壓結束後雖有長度比對（`:132-137`），但配置已經發生。
- **可重現的情境：** 任一存檔把 header `0x12` 的 u32 改成 `FF FF FF FF`。預期：配置約 4 GiB 清零記憶體；commit 額度足夠時，之後回 `DECOMPRESS`；額度不足時 `handle_alloc_error` 終止行程。
- **嚴重度：** low（多數機器上只是暫時性的 4 GiB；只有在 commit 額度不足時才 abort）。
- **確信度：** plausible（依賴行為已讀原始碼確認；後果沒有實際執行）。
- **來源：** 既有問題（`3188491`）。
- **建議方向：** 要求 `uncomp_size <= payload_size * 255 + 某常數`，或設一個遠大於實際存檔（目前數 MB）的上限。

### F-06 — ChaCha20 的 32 位元計數器跨界時 panic

- **位置：** `src/save/crypto.rs:55-63`；`chacha20-0.10.0/src/variants.rs:37-53`（IETF 變體 `Counter = u32`，`remaining_blocks = u32::MAX - block_pos`）；`cipher-0.5.1/src/stream/wrapper.rs:78-91`（需求區塊數大於剩餘時回錯）；`cipher-0.5.1/src/stream.rs:185-188`（`apply_keystream` 對錯誤 `.expect("end of keystream reached")`）。
- **問題與根因：** 存檔把 nonce 前 4 位元組當成初始區塊計數器，`seek(counter * 64)` 後一次處理整個 payload。只要 `counter + ceil(len/64) - 1 > u32::MAX`，`apply_keystream` 就 panic。這和 A-06 是同一個依賴語意，這次已讀原始碼確認 panic 行為。
  - 遊戲的 nonce 看起來是隨機產生的。16 位元組「counter ‖ nonce96」正是 OpenSSL `EVP_chacha20` 的 IV 格式，而 OpenSSL 在計數器溢位時會進位到下一個 word；如果遊戲用的是這類實作，計數器落在最後 ⌈len/64⌉ 個區塊內的**合法**存檔（機率約 `len / 64 / 2³²`，數 MB 的存檔約百萬分之幾）會被 crimson-rs 拒絕載入，而且即使不 panic 也解不出正確明文。遊戲實際用哪種語意，目前不知道。
  - 寫回路徑沿用原 nonce（`src/c_abi/mod.rs:4188`），body 變長時同樣可能在寫入時 panic。
  - C ABI 會把 panic 攔成 `PANIC`；Python 端看到 `PanicException`（`BaseException`，`except Exception` 接不到）。
- **可重現的情境：** 把任一存檔 header `0x1A` 的 4 位元組改成 `FF FF FF FF`，呼叫 `crimson_save_load_from_file`。預期回 `PANIC`（`crypto.rs:62`）。
- **嚴重度：** low。
- **確信度：** plausible（panic 語意已讀依賴原始碼確認；遊戲端語意與真實存檔命中率未驗證）。
- **來源：** 既有問題（`3188491`）。

### F-07 — 構造輸入可觸發的 panic

- **位置：**
  1. `src/save/body/decoder.rs:404-409`：marker 走訪遇到 present 的 list 時，只檢查 `raw[cursor] == 0` 就呼叫 `try_decode_object_list_at(raw, cursor, cursor, limit, …)`，沒有像 `decode_object_list`（`:1022`）那樣先檢查 `cursor + 18 <= limit`。後者的 `raw[body_cursor + 1..=3]`（`:1088-1090`）、`read_u32(raw, body_cursor + 4)`（`:1092`）、`read_u24`（`:1096`）、`raw[field_cursor..body_cursor + header_size]`（`:1147`）都假設這 18 位元組存在。
  2. `src/save/body/decoder.rs:1079-1082`：`marker_run_plus_zeros` 檢查 `marker_end + 17 <= tail_cursor`，接著切 `raw[marker_end + 5..marker_end + 18]`，需要的是 `+ 18`。差 1。
- **問題與根因：** 當 region 的結尾就是 `raw` 的結尾（最後一個 TOC block；`decoder.rs:793` 非 inline fallback 用 `raw.len()` 當 tail；`decode_one_list_element_bytes`（`:1292-1300`）用呼叫端給的 template 位元組長度當 tail）時，這兩處會索引越界而 panic。region 結尾不在 `raw` 結尾時不會 panic，但會讀到 region 之外 1 位元組（第 2 點），回傳的 `end` 超過 `tail_cursor`，欄位範圍和後面的欄位重疊，重編時那個位元組會被寫兩次。
- **可重現的情境：** 最後一個 TOC block 的 `data_offset = body_len - 10`、`data_size = 10`：`mbc = 1`、mask `01`、reserved 4 B，class 的第一個欄位是 `meta_kind = 6`，於是欄位從 `body_len - 3` 開始；把該位元組設為 0。marker 走訪進入第 405 行後在 `:1088` 或 `:1096` 讀到 `raw[body_len]` → panic。C ABI 回 `PANIC`。
- **嚴重度：** low（C ABI 與 PyO3 都攔得住一般 panic；`Cargo.toml` 沒有 `panic = "abort"`）。
- **確信度：** plausible。
- **來源：** 既有問題（第 1 點 `4518157`；第 2 點 `735b678`）。

### F-08 — `trailing_pad` 的 relocation 只處理「整個 block 平移」

- **位置：** `src/save/body/encoder.rs:107-129`（`:113-115` 在 `old_off == new_off` 時直接 return；`:123` 只比對 `old_off + p + 4`，`p` 是**新輸出**裡的位置）、`:66-77`。
- **問題與根因：** `trailing_pad`（以及 kind 5 的 `wrapper_prefix`、inline payload 的 pad）裡可能藏著沒解出的 wrapper `payload_offset`，這正是 `docs/save-loader-length-change-crash.md` 的遊戲 CTD 類型。relocation 假設 pad 在 block 內的相對位置不變：
  - 被編輯的 block 本身通常不搬家（前面的 block 沒變），`old_off == new_off`，pass 直接跳過；但編輯點之後的 pad 在 block 內已經位移，stale offset 沒被修正。
  - block 有搬家而且內部也變長時，stale 值是 `old_off + p_old + 4`，掃描找的是 `old_off + p_new + 4`，對不上。
  - 解出來的 locator / list wrapper 不受影響，它們由 `encode_locator_field` / `encode_list_element_wrapper` 依輸出位置重算（`:449`、`:498`），這部分是正確的。
  - 目前 `test_save_body_absent_markers_every_live_save` 斷言所有 live save 與 fixtures 的 `trailing_pad` 為 0（`src/lib.rs:907`），所以這條在已知存檔上是潛伏的；它會在 F-01 的漂移情境下（block 掉回舊走訪）重新出現。
- **可重現的情境：** 一個掉回舊走訪的 block B，`trailing_pad` 內有 self-referential offset；對 B 中位於 pad 之前的 list 做 `crimson_save_list_insert_element`。B 的 `data_offset` 不變 → relocation 不執行 → 寫出的存檔在遊戲讀檔時追到失效的 offset。
- **嚴重度：** low（目前潛伏；一旦觸發就是遊戲 CTD）。
- **確信度：** plausible。
- **來源：** 既有問題（`e6d000c`、`8b06ed5`）。
- **建議方向：** 在 pad 裡記錄「pad 內的 offset 指向 pad 起點 + k」的相對關係，依 pad 的新絕對位置重寫；或直接拒絕編輯含非空 pad 的 block（與 F-01 的建議一致）。

### F-09 — 版本漂移不會明確失敗（審計重點 3）

- **位置與行為：**
  - **未知 `meta_kind`**：marker 走訪回 `None`（`decoder.rs:424`）；頂層舊走訪 `_ => break 'outer`（`:292`），其後欄位全是 `Unknown`、位元組進 `undecoded_ranges`；inline payload 舊走訪回 `Err`（`:979-984`），使整個 child 變成 placeholder（`:1205-1242`）。三者都沒有向呼叫端回報「schema 出現新種類」，`crimson_save_load_from_file` 照樣回 `OK`。`meta_size == 0` 的 kind 0/1/2/3 同樣落到這些分支。
  - **mask byte count 超出 1..=16**：用 schema 推算的值取代（`:61-67`），之後讀 mask 的位置是猜的；encoder 寫回的是推算值（`encoder.rs:250`），no-op 重編就改變位元組。
  - **未知 `class_index` / block 解不出**：`decode_blocks` 用 `filter_map` 靜默略過（`:38-47`）。讀取 API 看到的是殘缺的 blocks；之後的 encode 因 `blocks.len() != toc.entries.len()` 失敗（`encoder.rs:157-166`），這一點是明確失敗。
  - **TOC 截短**：`entries` 少於 `toc_count` 時靜默停止（`toc.rs:64-69`）。
  - **list 部分解碼**：舊走訪接受 `count != elements.len()`（`decoder.rs:1122-1139`），見 F-01 第 4 點。
  - **`zero4_count_u32` 與 `zero1_count_u24` 的判定**：兩者都是 18 B header。`zero1` 把 `+1..+4` 當 u24 count；`zero4` 在 `+1..+3` 為 0 時改讀 `+4..+8`，其中 3 B 屬於後面 13 B 的未知欄位（`:1087-1098`）。count = 0 的 list 若那 3 B 非零，count 會被讀成大數。這 13 B 的意義沒有文件。
  - **count = 0 的 list**：舊走訪對 leading-pad 0..4 取「走最遠」的結果（`:1041-1058`），count = 0 時每個位置都成功，最遠的會吞掉下一欄位最多 4 B（`src/c_abi/mod.rs:3024-3041` 的註解記錄了這個歧義）。marker 走訪不做 probing，所以只影響舊走訪。
- **問題與根因：** decoder 刻意採「解 95% 比整個失敗好」的策略（`decoder.rs:19-23`），讀取時這是合理的；但它沒有把「這份 body 有猜測成分」傳給寫入路徑（F-01），也沒有給呼叫端一個旗標。計畫書的跨區塊要求是「只追最新版的解析器要能明確失敗，不能悄悄地讀錯」。
- **可重現的情境：** schema 中某欄位的 `meta_kind` 改成 9；`crimson_save_load_from_file` 回 `OK`，該 block 的 JSON（`crimson_save_get_block_json`）出現非空 `undecoded_ranges`，但沒有任何錯誤碼。
- **嚴重度：** low（單獨看只影響讀取的正確性；寫入側的後果算在 F-01）。
- **確信度：** confirmed（程式碼行為）／plausible（實際漂移時的表現）。
- **來源：** 既有問題（`735b678`、`61dd9cf`、`f1513b8`）。
- **建議方向：** 在 handle 上提供「decode 完整度」查詢（undecoded 位元組數、placeholder 數、舊走訪 block 數），或在載入時以獨立錯誤碼回報。

### F-10 — HMAC 失敗仍可載入，寫回時無條件重新封裝（審計重點 4）

- **位置：** `src/save/io.rs:19-23, 126, 139`（記錄 `hmac_ok` 後照樣回傳 body）、`:152-174`（寫回一律重算 HMAC）；`src/c_abi/mod.rs:1392-1398`（`crimson_save_get_hmac_ok`）、`:4167-4199`（`write_to_file` 不看 `hmac_ok`）；`src/python.rs:1585-1589`（`write_save_with_nonce` 直接設 `hmac_ok: true`）。
- **問題與根因：**
  1. 允許載入是刻意的（文件說明是沿用 Python 參考實作），C ABI 也提供 `get_hmac_ok` 讓 C# 端判斷，這部分符合計畫書描述。
  2. 寫回時一律以新內容重算 HMAC，不論載入時是否通過。一個因磁碟錯誤或被第三方工具改壞而 HMAC 不符的存檔，原本遊戲會拒絕載入；經過編輯器開啟、任意修改、儲存後，就成了 HMAC 正確、內容仍是壞的存檔。C ABI 沒有選項可以拒絕寫回 HMAC 失敗的存檔，也沒有在寫入時回報。
  3. 寫回沿用原 nonce（`src/c_abi/mod.rs:4188`），同一組 key + nonce 加密了兩份不同明文。key 本身是公開常數，所以沒有機密性影響，只記錄。
- **可重現的情境：** 翻轉某存檔加密 payload 中一個位元（HMAC 失敗、LZ4 仍解得開），`crimson_save_load_from_file` → `OK`，`get_hmac_ok` → 0；`crimson_save_write_to_file` → `OK`，重新載入後 `get_hmac_ok` → 1。
- **嚴重度：** low（需要 C# 端忽略 `hmac_ok`；後果是讓遊戲的完整性檢查失去作用）。
- **確信度：** confirmed（程式碼）／plausible（實際情境）。
- **來源：** 既有問題（`3188491`、`37d5fa3`）。

### F-11 — Header / TOC 的 offset 與 size 只防越界，不防不一致（審計重點 5）

- **位置：** `src/save/io.rs:102-116, 123`（payload 範圍有 `checked_add`，但 `payload_end` 之後的位元組被忽略，寫回時消失）；`src/save/body/toc.rs:58-60`（`prefix_zero`、`stream_size` 從不驗證）；`src/save/body/decoder.rs:55-59`（block 範圍用 `min(raw.len())` 截斷，不檢查落在 schema / TOC 區域內、不檢查重疊或順序）、`:793`（非 inline fallback 可讀 body 任意位置，包括 prefix、schema、TOC）；`src/save/body/encoder.rs:180-200, 222-244`（從 `schema_end + toc_total` 起把所有 block 依 TOC 順序連續排列，`stream_size` 寫成結尾的絕對位置）。
- **問題與根因：**
  - 解碼端所有讀取都有界（64 位元上沒有整數溢位：u32 + u32 轉 `usize` 相加不會溢位），所以這裡沒有越界讀取。問題在一致性：兩個 TOC entry 指向重疊範圍時，encoder 會把重疊位元組各寫一次；block 不依 offset 排序、block 間有間隙、最後一個 block 之後還有資料、或 payload 之後還有位元組時，寫回都會靜默改變佈局。真實存檔目前滿足這些假設（`test_save_body_full_roundtrip` 在 live save 上長度不變），所以影響限於損壞或構造的檔案。
  - Encoder 的 `as u32`（`encoder.rs:197, 200, 236, 449, 498`；`io.rs:165-166`）在超過 4 GiB 時靜默截斷，`block_data_offset + out.len() + 4` 在 debug 下可能溢位 panic。從檔案路徑進來的 body 受 `uncomp_size: u32` 限制，到不了；只有 Python `write_save_with_nonce`（任意 `body`）可能觸及。
- **可重現的情境：** 把某存檔第 2 個 TOC entry 的 `data_offset/data_size` 改成和第 1 個相同；載入成功，任何改長度的編輯後 body 會多出一份第 1 個 block 的內容。
- **嚴重度：** low。
- **確信度：** plausible。
- **來源：** 既有問題（`ecf1c10`、`43242f0`）。

### F-12 — `scalar_from_bytes` 與 `encode_scalar` 不驗證長度

- **位置：** `src/save/body/object.rs:300-360`（`pub fn scalar_from_bytes`，經 `src/save/mod.rs:48-51` 公開）：size 1 時 `data[0]`（`:305, 346, 354`）在空切片上 panic；size 2/4/8 時 `try_into().unwrap_or([0; N])`（`:338-357`）在長度不符時靜默得 0。`src/save/body/encoder.rs:577-589`：`ScalarValue::Bytes` 長度不等於 `meta_size` 只有 `debug_assert`，release 下照寫，block 長度隨之改變。
- **問題與根因：** decoder 自己呼叫時長度一定吻合；c_abi 的 scalar setter 也先檢查（`src/c_abi/mod.rs:1749-1753`）。但 `element_template.rs:424` 用 `bytes.len()` 而非 `def.meta_size` 當 size 傳入，長度合約落在呼叫端，函式本身不防。
- **可重現的情境：** `scalar_from_bytes(&[], "uint8", 1)` → panic；`scalar_from_bytes(&[1,2,3], "uint32", 4)` → `U32(0)`。
- **嚴重度：** nit。
- **確信度：** confirmed（程式碼）／plausible（目前有無呼叫端觸發）。
- **來源：** 既有問題（`735b678`、`43242f0`）。

### F-13 — 文件與程式碼脫節

- **位置與內容：**
  1. `src/save/body/toc.rs:10` 說 `stream_size` 是「total data section size」；encoder 寫的是 body 結尾的**絕對位置**（`encoder.rs:195-200`）。c_abi 的 clone→remove 測試斷言重編後與原始 body 位元組一致（`src/c_abi/mod.rs:5936-5939`），若該測試在維護者機器上通過，錯的是 toc.rs 的說明。
  2. 「no-op roundtrip 是否位元組一致」有三種互相矛盾的說法：`src/save/body/mod.rs:108-110` 與 `encoder.rs:13-17, 61-65` 說一致；`src/lib.rs:1064-1073, 1216-1222` 說引擎寫了非正規的 `payload_offset`，第一次編碼一定不同；`src/c_abi/mod.rs:5936-5939` 與兩個 faction 測試又斷言一致。`decoder.rs:774-779` 說「observed in FactionSaveData list elements」的非正規 offset，和 faction 測試的「byte-identical」也相衝突。需要在有 live save 的機器上釐清哪個是現況。
  3. `decoder.rs:115` 與 `object.rs:263` 引用 `docs/save-body-format.md`，該檔不存在（`docs/` 下也沒有對應的條目）。
  4. `src/save/io.rs:156-157` 說「Use the same high-compression level as the Python writer (compression=9)」，但 `lz4_flex::block::compress` 是一般壓縮器；`src/lib.rs:629-633` 的測試註解也承認輸出與遊戲不同。`src/save/mod.rs:34-36` 說「unmodified save round-trips byte-for-byte」，與同一個測試註解矛盾。
  5. `src/save/io.rs:29` 說 `TooSmall` 是「header + at least one body byte」，實際只檢查 header（`:80`）。
  6. `docs/c-abi-surface.md` 自稱「Full C-ABI bridge catalog」，但沒有列出存檔核心入口 `crimson_save_load_from_file`、`crimson_save_free`、`crimson_save_write_to_file`、`crimson_save_get_hmac_ok`（`src/c_abi/mod.rs:823, 874, 4167, 1392`），只有 `:424` 一行 `save/` 目錄說明。（這組函式屬 G 區塊，記在這裡是因為跨區塊要求文件一致性。）
- **嚴重度：** nit。
- **確信度：** confirmed（文字不一致）；第 2 點「現況是哪一種」未驗證。
- **來源：** 既有問題。

### F-14 — 被 `allow(dead_code)` 遮住的 dead code

- **位置：** `src/save/body/decoder.rs:1302-1324`（`scalar_as_i128`，`#[allow(dead_code)]`；全 repo 沒有呼叫端，任何 feature 組合下都是 dead code）；`src/save/body/encoder.rs:3-7`（模組級 `#![allow(dead_code)]`，理由是「Phase B.1 … no in-crate caller」，但 c_abi 早已使用）；`src/save/body/object.rs:92-98`、`src/save/body/mod.rs:113-115`（同樣的過時理由）。
- **問題與根因：** 計畫書要求「`cfg_attr(allow(dead_code))` 不能掩蓋真正的 dead code」。`save` 是私有模組（`src/lib.rs:78`），bare default build 下整個模組本來就只被測試用到，由 crate 級 `cfg_attr` 處理（設計如此）；但上述 item 級／模組級的 allow 在 `c_abi,python` 全 feature 的 clippy 下也生效，使 CI 抓不到真正沒用的 `scalar_as_i128`，也讓 encoder 日後新增的死函式不會被發現。
- **嚴重度：** nit。
- **確信度：** confirmed。
- **來源：** 既有問題（`735b678`、`43242f0`）。

---

## 審計重點逐條對照

| 審計重點 | 結論 |
|---|---|
| 1. Encoder relocation；有沒有「改長度後 encode 再 decode」的對照測試 | 解出的 locator / list wrapper 的 `payload_offset` 依輸出位置重算，block 內外位移都正確（`encoder.rs:449, 498`）。改長度後再 decode 的測試存在（c_abi clone/insert/remove 系列），但 decoder 忽略 `payload_offset`，所以這類測試抓不到 relocation 錯誤；沒有獨立不變式檢查（F-04）。`trailing_pad` 內的 offset 只處理整塊平移（F-08）。 |
| 2. Absence marker 讀寫對稱（缺席／存在但空／存在且非空） | Marker 走訪下對稱：缺席寫 `0x01`（`encoder.rs:305-308` ↔ `decoder.rs:357-367`），存在時 dynamic array 為 `00 + u32 count`（count 0 合法，`decoder.rs:438-467`），list 為 `00` 開頭的 18 B header 原樣保存。這由 `test_save_body_absent_markers_every_live_save` 在 live save 與 fixtures 上釘住（`src/lib.rs:824-914`）。舊走訪下 marker 混在相鄰欄位裡，位元組仍保得住；count = 0 的 list 有偏移歧義（F-09）。沒有離線合成測試覆蓋三種狀態（F-04）。 |
| 3. 未知 `FieldKind` / meta kind | 不會 panic，但也不會明確失敗：退回舊走訪、留下 `Unknown` / undecoded / placeholder，載入回 `OK`（F-09），寫回時靜默丟位元組（F-01）。 |
| 4. HMAC 失敗是否允許載入、寫回是否重算 | 允許載入（`hmac_ok = false`，C ABI 可查）；寫回一律重算，不看 `hmac_ok`（F-10）。 |
| 5. Header / TOC 的 offset、size：越界、重疊、溢位 | Header payload 範圍與 schema 讀取都有正確的越界檢查；TOC 迴圈有長度檢查但配置在前（F-02）；`uncomp_size` 驅動大配置（F-05）；重疊、順序、間隙、`stream_size` 都不驗證（F-11）；decoder 有兩處越界 panic（F-07）。64 位元下沒有可觸及的整數溢位。 |

## 跨區塊關注點

| 主題 | 結論 |
|---|---|
| 不可信輸入的配置上限 | `toc.rs:63`（F-02，可 abort）、`io.rs:129` 經 `lz4_flex`（F-05）。`schema.rs:83, 89` 的 `with_capacity` 以 u16 為上限（`TypeDef` / `FieldDef` 約 70 B，最多約 4.7 MB），可接受。decoder 的 `count` 都在讀到位元組時才 push，沒有預先配置。 |
| Panic 面與 abort | 一般 panic：F-06、F-07、F-12（C ABI 攔得住，回 `PANIC`；`Cargo.toml` 沒有 `panic = "abort"`）。Abort：F-02（配置失敗）、F-03（stack overflow），`catch_unwind` 擋不住。`header.rs` 的 `try_into().unwrap()` 都是固定長度切片，不會失敗。 |
| Roundtrip 對稱性 | 結構層的 no-op roundtrip 在已知存檔上由長度與冪等測試守住，但「與原始位元組一致」沒有在核心測試裡斷言，文件說法互相矛盾（F-04、F-13）。容器層（LZ4 + ChaCha20）只保證解出相同 body，不保證檔案位元組一致（`src/lib.rs:625-655`），這是已知且合理的取捨，但與 `src/save/mod.rs:34-36` 的說明不符。 |
| 版本漂移明確失敗 | Save 版本（`derive_key` 只接受 1、2，其餘回 `UnsupportedVersion`）與 body magic 會明確失敗；schema 自帶描述，所以欄位增減本身不是問題；但「decoder 解不完整」不會被回報，寫入時還會擴大成資料損毀（F-01、F-09）。 |
| Feature gate | `src/save/mod.rs:52-65` 把只給 c_abi 用的 re-export 用 `#[cfg(feature = "c_abi")]` 包起來，`python.rs` 只用無條件公開的 item，四種組合下的可見性看起來一致（本回合沒有實際編譯）。問題只在 item 級 `allow(dead_code)` 遮住真正的 dead code（F-14）。 |
| `docs/c-abi-surface.md` 一致性 | 存檔核心的四個入口沒有列在 catalog 裡（F-13 第 6 點）。 |

---

## 排除說明（查證後沒有問題的項目）

計畫書的「掃描時注意到的待查事項」**沒有分配給 F 區塊的項目**，所以這裡列的是審計重點與跨區塊關注點中，查證後確認沒有問題的部分：

- **E-01 Header 與 payload 範圍：** `Save::parse` 先檢查長度 ≥ 128（`io.rs:80`），`payload_end` 用 `checked_add` 並與檔案長度比較（`:102-116`）；`SaveHeader` 的存取都是固定 128 B 陣列上的固定區間，`unwrap()` 不會失敗。
- **E-02 金鑰推導：** v1 前綴 25 B、v2 前綴 19 B，加上 25 B 的後綴都 ≥ 31 B，`material[i]`（`crypto.rs:46`）不會越界；不支援的版本回 `None` → `UnsupportedVersion`，是明確失敗。
- **E-03 HMAC 驗證：** 用 `verify_slice`（常數時間），計算對象是解密後、解壓前的資料，與模組文件一致；寫回時以同一個 key 重算。
- **E-04 Schema 解析：** 所有讀取經 `check_remaining`（`saturating_add`），字串長度 u32 也先檢查剩餘位元組；`type_count = 0` 時不會觸發 `type_count - 1` 的下溢（迴圈不執行）。
- **E-05 Decoder 的 dynamic array / inline bytes：** `decode_tagged_dynamic_array` 用 `checked_mul` / `checked_add`；`decode_inline_bytes` 用 `saturating_mul` / `checked_add`；舊版四種 header 變體逐一追過，每個索引在使用前都有 `<= tail_cursor` 的檢查，而 `tail_cursor <= raw.len()`。
- **E-06 `collect_decoded_ranges` 的 u32 加法（`decoder.rs:514, 522`）：** child / element 的 `data_offset + data_size` 等於其結尾位置，不超過 `raw.len()`，而 body 長度受 `uncomp_size: u32` 限制，所以不會溢位。
- **E-07 Inline payload 的 trailing-size probe：** `probe <= tail_cursor - 4` 且 `probe >= cursor >= payload_start`，`read_u32` 與減法都不會越界或下溢（效能問題記在 F-03）。
- **E-08 Encoder 重算的欄位：** TOC 的 `data_offset` / `data_size` / `toc_count` / `stream_size`、inline payload 的 trailing size、解出的 wrapper `payload_offset` 都從輸出重新計算，不信任舊值；`encode_body` 的兩段式重編在 block 搬家時會用新 offset 再編一次。
- **E-09 Scalar 往返：** `Bool` 保存原始位元組（`0x01` / `0xFF` 都保得住）；浮點數用 `from_le_bytes` / `to_le_bytes`，位元不變（NaN payload 也保留）。
- **E-10 寫入與 batch：** `crimson_save_write_to_file` 在 deferred batch 開啟時回 `BATCH_IN_PROGRESS`，不會寫出過時的 body。
- **E-11 Panic 策略：** `Cargo.toml` 沒有 `[profile] panic = "abort"`，C ABI 與 PyO3 的 panic 攔截對一般 panic 有效（與 A 區塊結論一致）。

## 未能驗證、建議在有 fixtures 與 live save 的機器上補做

| 項目 | 建議方式 | 相關 finding |
|---|---|---|
| F-02、F-05、F-06、F-07 的實際行為 | 用構造的 body / header 呼叫 `Save::parse` / `Body::parse` 的最小重現（不寫入任何使用者存檔） | F-02、F-05、F-06、F-07 |
| F-03 的 stack 深度與時間 | 構造自我參照與深鏈 body，在獨立行程裡載入 | F-03 |
| 漂移時 encoder 是否真的丟位元組 | 把 fixture 的某個 block 改成舊走訪無法完全解出的形狀，對另一個 block 做 `list_insert_element`，比較前後該 block 的位元組 | F-01、F-08 |
| no-op 重編是否與原始 body 位元組一致；`stream_size` 的真實語意 | 對 `crimson-rs-private` 的 1.09、1.10 與 length-change fixtures 和目前的 live save 執行 `Body::write(decode_blocks(raw)) == raw` | F-04、F-13 |
| 真實存檔的 nonce 計數器分布 | 只讀 header `0x1A..0x1E`，不必解密 | F-06 |
