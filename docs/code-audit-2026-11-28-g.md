# Code audit — G. 存檔編輯 C ABI（2026-11-28）

依 [`code-audit-2026-11-28-plan.md`](code-audit-2026-11-28-plan.md) 的 G 區塊做靜態審計。**沒有修改任何程式碼**，沒有執行 crimson-rs 的任何程式或測試，沒有解密、解析或寫入任何存檔，也沒有碰 PAPGT trailer（G 區塊不涉及 PAPGT）。沒有派發 subagent。

- **基準：** `dev` @ `e465942`。下文的行號都以這個 commit 為準。
- **範圍：** `src/c_abi/mod.rs` 的 1–76 與 809–4770（load / free、inventory 與 character ref 列舉、mutation version、scalar getter、block info / JSON、scalar 與 typed setter、batch、改長度的 list / presence / inline-bytes / dynamic-array 編輯、transplant、write、deferred redecode、全域鎖、JSON 序列化）、`src/c_abi/element_template.rs`、`src/c_abi/all_items.rs`、`src/c_abi/positions.rs`。`mod.rs:581-808` 在行號上屬於 B 區塊的範圍，但內容是存檔 handle 與 G 的 `repr(C)` 結構（`CrimsonSaveHandle`、`DeferredState`、`CrimsonBlockInfo`、`CrimsonPathStep`、三個 batch op、`CrimsonInventoryItemRecord`），所以一併審查。測試（`mod.rs:4771-9345` 與三個檔案的 `mod tests`）只讀來判斷涵蓋範圍。
- **為了判斷後果而讀的上游程式：** `src/save/body/decoder.rs:38-47`（`decode_blocks`）、`:150-204`（舊走訪）、`:326-431`（marker 走訪）、`:471-473`（placeholder 判定）、`:1205-1242`（placeholder 建構）、`:1293-1300`（`decode_one_list_element_bytes`）；`src/save/body/encoder.rs:255-322`；`src/save/body/object.rs:226-264, 300-360`；`src/save/body/mod.rs:71-128`；`src/save/mod.rs:43-65`；`src/lib.rs:12-93`。
- **對照的文件：** `docs/save-mutation-version.md`、`docs/save-deferred-redecode.md`、`docs/c-abi-surface.md:258-317`，以及各函式的 rustdoc。
- **方法：** 逐行閱讀每個 `extern "C"` 入口，追 caller 給的指標與長度、handle 狀態（`save.body` / `body` / `blocks` / `mutation_version` / `deferred_state`）在成功、失敗、panic 三條路徑上的變化，再與上述契約文件逐條比對。上游 F 區塊的結論（[`code-audit-2026-11-28-f.md`](code-audit-2026-11-28-f.md)）直接引用，不重複論證。
- **沒做到的事：**
  1. 計畫書要求 G 區塊以 fixtures 加 `cargo test --lib --features c_abi -- --test-threads=4` 驗證。本回合限定靜態審查，所以**沒有跑任何測試**，也沒有用存檔做位元組比對或構造重現。凡是行為後果都標 plausible，「未能驗證」一節列出建議的驗證方式。
  2. `git blame` 與 `git log -S` 在本 session 需要額外核准而沒有執行。來源歸屬改用 `git log -- <G 檔案>` 的 commit 訊息加程式內容推定。`95fa25f..e465942`（2.06 移植週期）對 G 檔案的改動只有 `mod.rs:77-345`（`sibling_header`、`EntrySources`、`merge_dlc_ffi`，屬 B 區塊）、新增 `pub mod faction_info;` 一行，以及 `element_template.rs` 的測試改讀 private fixtures，**都不在 G 的正式碼範圍內**。所以 **G 區塊的 finding 全部是既有問題**。

### 確信度與來源歸屬的定義

沿用 A、B、F 區塊報告的定義：

- **confirmed**：只用原始碼或文件文字就能確定的事實，例如「沒有這個檢查」、「沒有這組測試」、「文件和程式碼不一致」。
- **plausible**：行為後果是靠追程式路徑或依賴的已知語意推出來的，**沒有用位元組或實際重現驗證過**。依 CLAUDE.md 與計畫書，這類一律標 plausible，即使程式路徑本身是確定性的。
- **本次引入**：引入的 commit 在 `95fa25f`（2026-11-27）到 `e465942` 之間。G 區塊沒有這類。
- **既有問題**：引入時間早於 `95fa25f`。主要來源：`3eff882`（2026-05-12，scalar mutation + `write_to_file`）、`f1dd61c` / `df037b8`（block JSON）、`d4de625`（path setter）、`244e0cd`（scalar batch）、`1ba3051`（2026-05-14，改長度的 list 與 presence 編輯，B.2）、`f6dac92`（empty element + `list_insert_element`，B.3）、`b1a1079`（remove / presence batch，B.5）、`ad0f36c`、`d82e780`、`e5ecd10`（mutation version + inventory 列舉）、`a161064`（2026-05-16，deferred redecode）、`88ef779`、`21f1883`（`set_object_list_present`）、`698e221`（`all_items`）、`aeb2fa2`（`positions`）、`858cd30`、`501a493`（transplant）、`7191a7d`、`7e2cf87`（2026-06-10，全域鎖）、`4518157`（2026-09-18，absence marker 與 element template）。

---

## 摘要

| # | 標題 | 嚴重度 | 確信度 | 來源 |
|---|---|---|---|---|
| G-01 | `crimson_save_list_all_items` 與 `crimson_save_list_field_positions` 沒有取 `SAVE_FFI_LOCK`：與另一執行緒的改長度編輯並行時，讀到正被取代、釋放的 `blocks`（use-after-free） | high | confirmed（缺鎖）／plausible（後果） | 既有問題 |
| G-02 | 改長度編輯 commit 後的「re-parse」不是驗證：`decode_blocks` 不會失敗，`Body::parse` 只讀 schema 與 TOC。transplant 文件承諾的「定義不符 → `MUTATION_INVALID`」與 `list_insert_element` 的「解不出 → `BODY_PARSE`」都沒有實際保障 | high | confirmed（程式碼）／plausible（後果） | 既有問題 |
| G-03 | `set_object_list_present(1)` 不檢查 list 是否已 present：對已有 N 個元素的 list 呼叫，會丟掉全部元素，換成一個空元素，並回 `OK` | medium | confirmed（缺檢查）／plausible（後果） | 既有問題 |
| G-04 | Deferred batch 期間的改長度編輯不遞增 `mutation_version`，讀取 API 卻已看到新的樹；`save-mutation-version.md` 的「版本沒變，snapshot 就正確」在 batch 內不成立 | medium | confirmed（契約矛盾）／plausible（後果） | 既有問題 |
| G-05 | Mutation 途中 panic 沒有 rollback：`h.blocks` 與 `h.save.body` 失去一致；之後的 in-place scalar setter 可能用模板相對的 offset 改寫 body 前段 | medium | plausible | 既有問題 |
| G-06 | 改長度的 list 操作一律以 `elements.len()` 覆寫 `count`，不檢查原本是否 `count == elements.len()`，也不拒絕 placeholder 元素 | medium | confirmed（缺檢查）／plausible（後果） | 既有問題 |
| G-07 | 測試涵蓋：version 遞增測試只測 path setter；deferred 測試只測 clone 加 scalar；沒有並行、panic、deferred × 其他 mutation 的測試；所有 G 測試都只讀 live `slot107` | medium | confirmed | 既有問題 |
| G-08 | Deferred batch 內，剛被 toggle 成 present 的 scalar 或 template 建出的元素，其 `start/end` 為 0：之後同一 batch 的 `set_scalar_field*` 對正確長度回 `LENGTH_MISMATCH`，0 長度時寫入 0 或 panic | low | confirmed（程式碼）／plausible（後果） | 既有問題 |
| G-09 | List presence toggle 一律設 `absent_marker = !make_present`，不看原本的 absence 怎麼編碼；在舊走訪解出的 block 上，present(1)→present(0) 不是位元組一致 | low | plausible | 既有問題 |
| G-10 | Element template 的缺口：只比 kind 與 size、不比 `type_name`；接受 placeholder 元素；`meta_kind` 4 特例無法區分「來源缺席」與「新欄位」，且證據只到 2.03；zero4 list header 被改寫成 zero1 | low | confirmed（程式碼）／plausible（後果） | 既有問題 |
| G-11 | `list_insert_element`、transplant、template insert 都不檢查插入元素的 class 是否屬於目標 list | low | confirmed（缺檢查）／plausible（後果） | 既有問題 |
| G-12 | `crimson_save_write_to_file` 用 `std::fs::write` 直接覆寫目標，沒有暫存檔加 rename；中途失敗會毀掉使用者唯一的存檔 | low | plausible | 既有問題 |
| G-13 | `repr(C)` 結構的 layout 守門不完整：`CrimsonInventoryItemRecord`、`CrimsonCharacterRefRecord`、`CrimsonBlockInfo`、`CrimsonPathStep` 與三個 batch op（大小依指標寬度而變）都沒有 `const` assert、測試或 `c-abi-surface.md` 記載 | low | confirmed | 既有問題 |
| G-14 | 四個列舉 API 以 class / 欄位名稱、寫死的 1.07 `PLAYABLE_CHARACTER_KEYS` 與 40 B `Transform` 佈局判讀；改名時靜默回傳較少的紀錄，沒有失敗訊號 | low | confirmed（程式碼）／plausible（後果） | 既有問題 |
| G-15 | Transplant 不檢查 `target_handle != source_handle`，同一 handle 傳兩次即為 aliasing UB | nit | confirmed（缺檢查） | 既有問題 |
| G-16 | 文件與程式碼脫節：`c-abi-surface.md` 只列 47 個 `crimson_save_*` 符號中的 12 個；deferred 文件誤稱非 deferred batch 會部分套用；mutation-version 文件仍寫「single-threaded by contract」；多處 rustdoc 與實作不符 | nit | confirmed | 既有問題 |

沒有 critical。兩個 high 都是「API 自稱有保護、實際沒有」：G-01 是 `7e2cf87` 為了「C# 背景 bulk worker 與 UI 執行緒讀取重疊」加了全域鎖，卻漏掉 `all_items.rs`、`positions.rs` 兩個檔案（該 commit 只改了 `mod.rs`），而這兩個正是 UI 會頻繁呼叫的列舉 API。G-02 是 rustdoc 與錯誤碼文件把 commit 後的 re-decode 描述成驗證，但 `decode_blocks` 的型別是 `Vec<ObjectBlock>`，不可能失敗；加上 F-01（encoder 不拒絕沒解釋完的 block），呼叫端得到 `OK` 的同時，寫出的 body 可能已經壞了。

---

## Findings

### G-01 — 兩個列舉 API 沒有取全域鎖

- **位置：**
  - `src/c_abi/all_items.rs:706-739`（`crimson_save_list_all_items`，`:720-725` 直接 `&*handle` 後讀 `h.mutation_version` 與 `h.blocks`）。
  - `src/c_abi/positions.rs:515-548`（`crimson_save_list_field_positions`，`:529-534` 相同）。
  - 對照：`src/c_abi/mod.rs:4389-4410`（`SAVE_FFI_LOCK` 的說明：「Every save entry point takes this lock」，並點名「the C# app's background bulk-mutation worker overlapping a UI-thread block read … can free the blocks vec out from under an in-flight reader」）；`mod.rs` 內其他 43 個入口（含 `with_handle`）與 `element_template.rs:652, 720` 都有取鎖。
  - 次要：`src/c_abi/mod.rs:874-885`（`crimson_save_free`）也不取鎖。
- **問題與根因：** `7e2cf87` 的 commit 訊息寫「every save entry point (15 mutations, 10 reads, transplant, with_handle — 27 sites)」，`git show --stat` 顯示它只改了 `src/c_abi/mod.rs`。`all_items.rs`（`698e221`）與 `positions.rs`（`aeb2fa2`）都在 2026-05-17 加入，早於鎖，因此被漏掉；之後兩個檔案的修改（`f6696ef`、`1f32ec2`）也沒有補上。結果是這兩個入口在另一執行緒持鎖做改長度編輯時照樣執行：`apply_length_changing_mutation` 的 `h.blocks = new_blocks`（`mod.rs:1725`）或 in-place setter 的 `h.blocks = h.body.decode_blocks(...)`（`:1920, 2005, 2417`）會 drop 舊的 `Vec<ObjectBlock>`，而列舉函式正在走訪它。這是鎖要防止的 use-after-free，而且兩者同時持有 `&` 與 `&mut`，依 Rust 語意本身就是 UB。`crimson_save_free` 不取鎖則讓「關閉存檔時背景 worker 還在跑」同樣沒有保護；這比較接近 C 的 `free()` 契約（rustdoc 也這樣寫），所以只列為次要。
- **可重現的情境：** C# 端在背景執行緒對同一 handle 跑 `crimson_save_list_remove_elements_batch`（數十 ms），UI 執行緒同時為物品分頁呼叫 `crimson_save_list_all_items`。預期：偶發的存取違規（行程崩潰）、亂碼紀錄，或 `copy_nonoverlapping` 寫出已釋放記憶體的內容。
- **嚴重度：** high（UB 且可能讓 C# 宿主行程崩潰；觸發情境正是鎖的設計文件明列的使用方式）。
- **確信度：** confirmed（缺鎖由原始碼與 `7e2cf87` 的 diffstat 確定）／plausible（實際崩潰沒有重現）。
- **來源：** 既有問題（`698e221`、`aeb2fa2` 引入；`7e2cf87` 漏補）。
- **建議方向（不在本次修改）：** 在兩個函式的 `catch_unwind` 內第一行加 `let _ffi_guard = super::save_ffi_lock();`（`save_ffi_lock` 需改成 `pub(super)`）；`crimson_save_free` 也取鎖。再加一個 grep 式測試：每個 `extern "C" fn crimson_save_*` 的函式體都要出現 `save_ffi_lock` 或委派給有取鎖的函式。

### G-02 — 改長度 commit 後的 re-decode 不是驗證

- **位置：**
  - `src/c_abi/mod.rs:1707-1727`（`apply_length_changing_mutation`：只有 `Body::write` 或 `Body::parse` 失敗才回 `MUTATION_INVALID`）、`:4303-4321`（`crimson_save_end_deferred_redecode` 相同）。
  - `src/save/body/mod.rs:71-100`（`Body::parse` 只解析 prefix、schema、TOC；`decode_blocks` 回傳 `Vec<ObjectBlock>`）；`src/save/body/decoder.rs:38-47`（`filter_map` 靜默略過解不出的 block）。
  - 承諾驗證的文件：`mod.rs:141-144`（`MUTATION_INVALID`：「produced bytes the parser can't read back (e.g. … the re-parse failed)」）、`:2765-2769`（transplant：「On a definition mismatch the post-insert re-decode rejects the bytes with `MUTATION_INVALID` and the target is rolled back」）、`:2851-2853`（同上的程式註解）、`:3524-3525`（`list_insert_element`：「`bytes` must parse as a valid list element of a class known to the schema; else `BODY_PARSE`」）。
  - `src/c_abi/mod.rs:3572-3575` + `src/save/body/decoder.rs:1293-1300`：`decode_one_list_element_bytes` 丟掉 `_end`，而且 child 解不出時 `decode_object_list_element` 回傳 `Ok(placeholder)`（`decoder.rs:1205-1242`）。
- **問題與根因：**
  1. **Transplant：** 被移植的元素帶著來源 save 的 `DecodedField`（名稱、`meta_kind`、`meta_size`、mask 位元都是來源 schema 的），encoder 依這些資訊寫出位元組；commit 後以目標 schema 重新解碼。兩邊的欄位定義不同時，marker 走訪會失敗、退回舊走訪，留下 `Unknown` / undecoded / placeholder，**但不會有任何錯誤**：`decode_blocks` 沒有失敗路徑，`Body::parse` 根本不看 block 內容。所以文件承諾的 `MUTATION_INVALID` 加 rollback 永遠不會發生，呼叫回 `OK`，目標 save 多了一個解不出的元素。下一次任何改長度編輯會把這份解錯的樹再編一次（F-01），寫壞的範圍可能擴大。
  2. **`list_insert_element`：** 呼叫端給的位元組若 child payload 解不出，得到的是 `Ok(placeholder)`，不是 `BODY_PARSE`；位元組比元素長時多出的尾段被靜默忽略。插入 placeholder 後 encoder 依 F-01 第 2 點把元素重寫成 `25 + mbc` 位元組的空殼，原本的內容消失，呼叫回 `OK`。
  3. **block 被 `filter_map` 丟掉：** 若新 body 的某個 block 解不出（例如 encoder 寫出的位元組讓那個 class 解碼失敗），`h.blocks` 比 TOC 少一個，之後所有 `block_idx` 都錯位一格；而下一次改長度編輯會因 `blocks.len() != toc.entries.len()`（`encoder.rs:157-166`）永久回 `MUTATION_INVALID`。commit 本身仍回 `OK` 並遞增版本。
- **可重現的情境：** 取 1.10 fixture 的一個 `MercenarySaveData` 元素（帶 `_occupationState`），用 `crimson_save_transplant_list_element` 移植到 2.06 live save 的 `_mercenaryDataList`。依文件應得 `MUTATION_INVALID`；依程式碼預期得 `OK`，新元素在 `crimson_save_get_block_json` 裡出現 `<unknown>` 欄位或非空的 `undecoded_ranges`。（`element_template.rs` 的模組文件 `:7-10` 自己也寫了「an element captured on one patch cannot be inserted into a save another patch wrote」，但 transplant 的 rustdoc 沒有改。）
- **嚴重度：** high（呼叫端依文件信任的安全網不存在，後果是回 `OK` 的靜默寫壞存檔；transplant 與 insert 都是 CrimsonAtomtic 的跨存檔、匯入類功能會用到的入口）。
- **確信度：** confirmed（`decode_blocks` 的型別、`Body::parse` 的範圍、`_end` 被丟棄、placeholder 走 `Ok`，皆由原始碼確定）／plausible（實際跨版移植的位元組結果沒有驗證）。
- **來源：** 既有問題（`1ba3051` commit 路徑；`f6dac92` insert；`501a493` transplant 文件）。
- **建議方向：** commit 前後做真正的自我檢查，例如「新 blocks 數等於 TOC 數」、「新 body 沒有比舊 body 多出 `Unknown` 欄位、undecoded range 或 placeholder」、「`Body::write(new_blocks) == new_body`」；`decode_one_list_element_bytes` 要求 `_end == bytes.len()` 並拒絕 placeholder；transplant 改為比對兩邊 schema 中所有相關 class 的欄位定義，不一致就回錯（或直接導向 element template）。

### G-03 — `set_object_list_present(1)` 會清空已存在的 list

- **位置：** `src/c_abi/mod.rs:3245-3302`（`toggle_one_object_list_presence_in_place`：`:3267-3292` 不看 `field.present`，直接把 `value` 換成 `count = 1` 加一個預設元素）、`:3121-3142`（make-present 前先 `resolve_object_list_element_class`，`:3203-3239` 掃到的第一個「同 parent class、該欄位非空」的 block 很可能就是目標自己，所以不會失敗）。
- **問題與根因：** 函式名稱與文件（`:3053-3088`、`docs/c-abi-surface.md:280-286`）都把它定位成「toggle absent ObjectList fields」，但程式沒有拒絕已 present 的 list。對照 scalar 版的文件（`:2902-2905`）把「已經 present 再 present」描述為 no-op，呼叫端很容易以為 list 版也一樣。實際上，對一個有 N 個元素的 `_itemDyeDataList` 呼叫 present(1)，N 個染色元素全部被丟掉，換成一個全欄位缺席的空元素，呼叫回 `OK` 並遞增版本。
- **可重現的情境：** 對一個已染色物品（`_itemDyeDataList` count = 3）的 list 欄位呼叫 `crimson_save_set_object_list_present(h, b, path, n, field, 1)`。預期：回 `OK`，之後 JSON 顯示 `[1 elements, variant=zero1_count_u24]`，三筆染色資料消失。
- **嚴重度：** medium（單次呼叫即造成使用者資料遺失；需要呼叫端誤用，但 API 沒有提供任何防護，文件也暗示是冪等的）。
- **確信度：** confirmed（缺檢查）／plausible（實際存檔上的結果）。
- **來源：** 既有問題（`21f1883`）。
- **建議方向：** make-present 時若欄位已 present 就回 `OK` 不做事，或回一個獨立錯誤碼；make-absent 對已缺席的欄位同理。

### G-04 — Deferred batch 期間版本號與讀取結果脫鉤

- **位置：** `src/c_abi/mod.rs:1684-1693`（deferred 時直接改 `h.blocks`，不遞增）、`:1895-1910`、`:1987-1998`、`:2345-2374`（scalar 在 deferred 時也不遞增）；`:4221-4223`（rustdoc：讀取 API 在 batch 期間看到 in-progress 的樹）；`src/c_abi/mod.rs:984-986`、`:1191-1193`、`all_items.rs:722-724`、`positions.rs:531-533`（snapshot 的 `out_version` 是 `h.mutation_version`）。
  - 契約：`docs/save-mutation-version.md:110-115`（「if the version is unchanged, the snapshot IS still correct」）、`:157-161`（「while a batch is open … no observer outside the current FFI call has seen it」）；`docs/save-deferred-redecode.md:63-65`（讀取 API 在 batch 期間看到最新狀態）、`:69-73`。
- **問題與根因：** 兩份文件互相矛盾：deferred 文件說讀取在 batch 期間「see the latest state」，mutation-version 文件卻說 batch 中的狀態「沒有任何外部觀察者看過」，並以此為由不遞增版本。實際上：
  1. batch 開始前拿到的 snapshot（版本 v），在 batch 內做了 `list_remove_element` 之後，`get_mutation_version` 仍回 v，snapshot 被判定為「仍然正確」，但其中 `item_element_idx` 已經錯位。
  2. batch 內重新列舉得到的 snapshot 也標 v，`end_*` 之後才變 v+1；若 batch 被 `abort_*`，版本回到 v（`:4356`），這份 batch 內的 snapshot 在 abort 後仍標 v，但它描述的是已被丟棄的樹。
  這兩種情況下，依文件建議的 C# `InventorySnapshot` 寫法（`save-mutation-version.md:87-108`）都會沿用錯位的索引，接著用 `set_scalar_field_path` 改到**別的元素**。
- **可重現的情境：** `list_inventory_items(&v)` → `begin_deferred_redecode` → `list_remove_element(container 3, item 0)` → 依舊 snapshot 對 `(3, 5)` 呼叫 `set_scalar_field_path`（版本比對仍相等）。預期：寫到原本的第 6 個物品。
- **嚴重度：** medium（會改錯欄位，屬靜默資料錯誤；需要呼叫端在 batch 內混用舊 snapshot，而文件明示這是安全的）。
- **確信度：** confirmed（文件互相矛盾、程式不遞增）／plausible（C# 端實際是否這樣使用）。
- **來源：** 既有問題（`a161064`）。
- **建議方向：** 二選一並讓文件一致：batch 內的每次改長度編輯也遞增（`abort_*` 不回退，改用另一個 epoch），或在 batch 開啟期間讓 snapshot API 回 `BATCH_IN_PROGRESS`（至少回傳一個「in-batch」旗標）。

### G-05 — Mutation 途中 panic 沒有 rollback

- **位置：** `src/c_abi/mod.rs:1699-1727`（`blocks_backup` 是區域變數，panic 時隨 unwind 被丟棄）、`:1916-1921`、`:2004-2006`、`:2411-2417`（in-place 先改 `save.body` 再 `decode_blocks`）、`:4412-4422`（`save_ffi_lock` 的說明：「the length-changing mutations roll their state back on error, so a poisoned lock should not wedge every later call」）；`:1607-1612`（`resolve_leaf_range` 只檢查 `leaf.end <= body_len`）。
- **問題與根因：** `catch_unwind` 在各入口的最外層，rollback 只寫在 `Err` 分支。panic 的可能來源包括：`decode_blocks` 在構造輸入或 encoder 產物上越界（F-07）、`scalar_from_bytes` 的 `data[0]`（G-08、F-12）、encoder 的 `debug_assert`（debug build）。依 panic 發生的時點：
  - mutator 內或 `Body::write` / `Body::parse` 時：`h.blocks` 已是改過的樹，`h.save.body`、`h.body` 仍是舊的。
  - in-place setter 的 `decode_blocks` 時：`h.save.body` 已改，`h.blocks` 是舊的。
  之後鎖被 `into_inner` 回收，handle 照常使用：讀取 API 顯示一半套用的狀態；`write_to_file` 寫出舊 body（mutation 消失）；下一次改長度編輯把上次一半套用的樹一起 commit。最糟的組合是：插入的元素還沒經過 re-decode，它的欄位 `start/end` 是相對於呼叫端模板位元組的小數字（例如 30..34），此時呼叫 `set_scalar_field_path` 改那個元素，`resolve_leaf_range` 只檢查上界，`h.save.body[30..34]` 被改寫，落在 body 的 prefix / schema 區。
- **可重現的情境：** 用 F-07 的構造方式讓某個 TOC block 在重新解碼時越界 panic，然後對任一 list 做 `crimson_save_list_insert_element`：回 `PANIC`；接著對新插入元素的 u32 欄位呼叫 `crimson_save_set_scalar_field_path`，再 `crimson_save_write_to_file`。預期：寫出的存檔 schema 區被改寫。
- **嚴重度：** medium（需要先有 panic；一旦發生，後續操作可能寫壞存檔的結構區，而鎖的說明讓人以為已有保護）。
- **確信度：** plausible。
- **來源：** 既有問題（`1ba3051`；`7e2cf87` 的 poison 回收說明）。
- **建議方向：** 在 `apply_length_changing_mutation` 與 in-place setter 內再包一層 `catch_unwind`，panic 時還原 `blocks` / `save.body`；或在 handle 上設「poisoned」旗標，之後的 mutation 與 write 一律回錯。

### G-06 — 改長度的 list 操作不檢查 list 是否完整解出

- **位置：** `src/c_abi/mod.rs:2490-2495`（remove）、`:2622-2627`（batch remove）、`:2690-2700`（clone）、`:2866-2871`（transplant）、`:3587-3592`（insert）、`src/c_abi/element_template.rs:741-746`（template insert）：全部是 `*count = elements.len() as u32` 後改寫 header。
- **問題與根因：** Marker 走訪只接受 `elements.len() == count` 且沒有 placeholder 的 list（`decoder.rs:413-418`），但舊走訪接受部分解碼（F-01 第 4 點、F-09）。G 的六個入口都沒有檢查 `*count as usize == elements.len()`、也沒有拒絕含 placeholder 的 list，就以 `elements.len()` 改寫 count：原本 count = 10、只解出 7 個的 list，remove 一個之後 header 寫成 6，而後面 3 個元素的位元組仍留在下一個欄位裡，遊戲讀到的 list 少了 4 個元素、下一個欄位開頭多了 3 個元素的位元組。clone 一個 placeholder 元素時，encoder 會依 F-01 第 2 點把它寫成空殼。
- **可重現的情境：** 漂移情境：某個 patch 讓 `InventoryElementSaveData` 掉回舊走訪，`_itemList` 只解出前 7 個元素；使用者在編輯器刪除其中一個物品 → 回 `OK`，寫出的存檔該容器的 count 與內容錯位。
- **嚴重度：** medium（漂移時的靜默資料毀損，G 層一行檢查即可攔下）。
- **確信度：** confirmed（缺檢查）／plausible（後果）。
- **來源：** 既有問題（`1ba3051`、`f6dac92`、`501a493`、`4518157`）。
- **建議方向：** 共用一個前置檢查：`count == elements.len()`、沒有 placeholder 元素、`header_variant` 屬於 marker 走訪接受的形狀，否則回 `LIST_VARIANT_UNSUPPORTED` 或新的錯誤碼。

### G-07 — 測試涵蓋

- **位置：** `src/c_abi/mod.rs:4851-4864`（`find_save` 只找 `%LOCALAPPDATA%\…\slot107`）、`:8417-8537`（`c_abi_mutation_version_bumps_on_mutation_only`）、`:8875-9345`（四個 deferred 測試）；`all_items.rs:846-862`、`positions.rs:682-708`（`CRIMSON_LIVE_SAVE` 或 `slot107`）；`element_template.rs:976-1046`。
- **問題與根因：**
  1. `save-mutation-version.md:140-143` 規定每個新 mutation 入口都要有「遞增恰好一次」的測試，但唯一的版本測試只測 `set_scalar_field_path` 與一個失敗的 `set_scalar_field`。改長度編輯、presence toggle、inline bytes、dynamic array、transplant、template insert、三個 batch、`end_*` / `abort_*` 的版本行為都沒有專門斷言（deferred 測試有測 `end` / `abort`）。
  2. Deferred 測試只用到 `list_clone_element` 與 `set_scalar_field_path`（以 `grep` 統計 `:8875-9345` 的呼叫）。G-04、G-08 的情境（batch 內讀取、toggle 後設值、template insert 後設值、remove batch、presence batch、inline bytes、dynamic array、object list present、transplant）沒有任何測試。`save-deferred-redecode.md:171-180` 宣稱「committed body bytes from a deferred batch are byte-identical to running the same mutations in normal mode」，只由 clone 加 scalar 兩種序列釘住。
  3. 沒有並行測試（G-01 會被一個「兩執行緒交錯呼叫」的壓力測試抓到）、沒有 panic 後狀態的測試（G-05）、沒有 G-03 / G-06 的負面測試。
  4. 改長度編輯的離線測試只有 `update_marker_run_count_patches_tail_u32`（`:6031`）與 element template 的三個合成測試；其餘全部依賴 live `slot107`，CI 與只有 fixtures 的機器上都以 `return` 算成 pass（與 F-04 相同的結構性問題）。`find_save` 的註解（`:4852-4854`）仍寫「the current Crimson Desert 1.12 save」，遊戲已到 2.06。
- **可重現的情境：** 在沒有 `slot107` 的機器上執行 `cargo test --lib --features c_abi -- --test-threads=4`：G 的 live 測試全部印出 `skipping …` 並 pass。
- **嚴重度：** medium（G 是直接寫使用者存檔的層；G-01、G-08 都是一個測試就能抓到的回歸）。
- **確信度：** confirmed。
- **來源：** 既有問題。
- **建議方向：** 讓 `find_save` 也接受 `test_fixtures::save(...)`；為 deferred 加「每個 mutation 入口各跑一次 normal 與 deferred，比對 body」的表格式測試；加一個多執行緒對同一 handle 交錯呼叫所有 `crimson_save_*` 讀取 API 的測試。

### G-08 — Deferred batch 內對新 present 的 scalar 設值會失敗或寫錯

- **位置：** `src/c_abi/mod.rs:1738-1756`（`apply_scalar_mutation_in_blocks`：`expected = field.end - field.start`）、`:3011-3015`（toggle 成 present 時不更新 `start/end`，註解寫「they'll be refreshed by the re-decode」）、`src/c_abi/element_template.rs:568-570`（template 建出的每個欄位 `start = end = 0`）；`src/save/body/object.rs:305, 346, 354`（size 1 時 `data[0]`）、`:338-357`（長度不符時 `unwrap_or([0; N])`）。
- **問題與根因：** 非 deferred 模式下每次改長度編輯後都會 re-decode，`start/end` 會更新；deferred 模式不會。原本缺席的 scalar 其 `start = end = 0`（`decoder.rs:170-178, 352-355`），toggle 成 present 後仍是 0；template 插入的元素所有欄位都是 0。之後在同一 batch 對這些欄位呼叫 `set_scalar_field*`：
  - 傳正確長度 → `LENGTH_MISMATCH`。
  - 傳 `bytes_len = 0` → 長度檢查通過；size 2/4/8 的欄位被靜默寫成 0 並回 `OK`；size 1（`bool`、`uint8`、`int8`）的欄位在 `data[0]` panic，回 `PANIC`。
  這違反 `save-deferred-redecode.md:171-180` 的「與 normal mode 位元組一致」。染色編輯器「`set_object_list_present` → `set_scalar_field_present` → `set_scalar_field_path`」與 dragon unlock「template insert → 改 `_mercenaryNo`」放進 deferred batch 時都會撞到。
- **可重現的情境：** `begin_deferred_redecode` → `set_scalar_field_present(…, field, 1, &[0;4], 4)`（原本缺席的 u32）→ `set_scalar_field_path(…, field, &[1,0,0,0], 4)`。預期回 `LENGTH_MISMATCH`；同樣序列在非 deferred 模式下回 `OK`。
- **嚴重度：** low（多數情況是明確的錯誤碼；靜默寫 0 需要呼叫端傳 0 長度）。
- **確信度：** confirmed（`start/end` 不更新、長度檢查的寫法）／plausible（C# 端實際序列）。
- **來源：** 既有問題（`a161064`；`1ba3051` toggle；`4518157` template）。
- **建議方向：** deferred 的 scalar 路徑改以 `field.meta_size` 為長度基準（`FixedPrefix` / `FixedSuffix` 的長度就是 `meta_size`），而不是 `end - start`。

### G-09 — List presence toggle 不看原本的 absence 編碼

- **位置：** `src/c_abi/mod.rs:3278-3282`（`field_mut.absent_marker = !make_present`）；`src/save/body/decoder.rs:170`（舊走訪一律 `absent_marker: false`）、`:357-367`（marker 走訪才設 `true`）；文件 `mod.rs:3073-3076`、`docs/c-abi-surface.md:284-286`（「present(1) → present(0) restores the original bytes」）。
- **問題與根因：** 位元組一致的保證只在 parent 由 marker 走訪解出時成立。parent 由舊走訪解出時（F-09 的漂移情境），原本缺席的 list 的 `0x01` 被吸進鄰近欄位或 header variant（`c-abi-surface.md:310-317` 的描述）；present(1) 時那個 `0x01` 仍留在鄰近欄位裡，等於多一個位元組；present(0) 再加一個 marker，結果比原本多 1 B。`ObjectBlock` 上沒有記錄「由哪種走訪解出」，所以 G 層也無法判斷。dynamic array（`meta_kind` 3）目前沒有 presence toggle，不受影響。
- **可重現的情境：** 讓某個含缺席 `_itemDyeDataList` 的 `ItemSaveData` 掉回舊走訪，呼叫 present(1) 再 present(0)，比較前後 body。預期多出 1 B。
- **嚴重度：** low（只在漂移時發生）。
- **確信度：** plausible。
- **來源：** 既有問題（`4518157`；present 路徑本身 `21f1883`）。
- **建議方向：** 在 `ObjectBlock` 記錄走訪方式；在舊走訪解出的 parent 上拒絕 presence toggle（與 F-01 的建議一致）。

### G-10 — Element template 的缺口

- **位置：** `src/c_abi/element_template.rs:110-121`（`template_from_block` 的拒絕條件）、`:403-406`（`meta_kind` 4 預設建立）、`:420-425`（scalar 只比 `meta_kind` 與 size）、`:459-473`（list header 一律以 `zero1_count_u24` 改寫 count）、模組文件 `:20-28`；`src/c_abi/mod.rs:3626-3649`（`build_empty_element_bytes` 讓所有欄位缺席，含 `meta_kind` 4）；`docs/c-abi-surface.md:294-296, 305-308`。
- **問題與根因：**
  1. **型別變更不攔：** 模組文件承諾「a field whose kind or size changed is `TEMPLATE_MISMATCH`, never a silent reinterpretation」。scalar 只比 `meta_kind ∈ {0,2}` 與 `meta_size`，不比 `type_name`；同寬度的型別變更（`int32` → `float`、`CharacterKey` → `ItemKey`）會把位元組原樣搬過去，正是被禁止的重新解讀。
  2. **接受 placeholder：** placeholder 元素（`decoder.rs:1222-1240`）有 wrapper、沒有欄位、`trailing_pad` 與 `undecoded_ranges` 都是空的，四個拒絕條件都不成立；`template_from_block` 會產生一個零欄位的模板，匯入後得到一個全缺席（加上預設 locator）的元素，原本的內容靜默消失。文件說它「Refuses an object the absence-marker walk did not fully explain」，但 `ObjectBlock` 沒有走訪方式的紀錄，四個條件只是近似。
  3. **`meta_kind` 4 特例：** 模板只記 present 欄位，所以無法分辨「來源 class 有這個欄位但缺席」與「來源 class 沒有這個欄位」；兩種情況都會在目標端建出 present 的空 child。這依賴「遊戲從不寫出缺席的 kind 4」這個統計結論，而 `c-abi-surface.md:305-308` 與模組文件記載的樣本是 1.10–2.03 的存檔，沒有 2.04–2.06。另外 `build_empty_element_bytes`（`list_insert_element`、`set_object_list_present` 用的空元素）把 kind 4 寫成缺席，與這個規則相反。
  4. **list header：** 來源若是 `zero4_count_u32`（`+1..+3` 為 0、count 在 `+4..+8`），以 `zero1_count_u24` 改寫會把 count 寫進 `+1..+3`，`+4..+8` 保留舊值，變成兩種形狀都不是的 header。真實存檔裡是否有 count > 0 的 zero4 list 未知（F-09：這 13 B 的意義沒有文件）。
- **可重現的情境：** 第 2 點：在舊走訪解出、含 placeholder 的 list 上呼叫 `crimson_save_export_element_template` → 回 `OK`，模板只有 class name；匯入另一個存檔後得到空元素。第 1 點：合成 schema 把 `_level` 從 `int32` 改成 `float`，`builder.build` 不回錯。
- **嚴重度：** low（第 1、3、4 點需要特定的 schema 變化；第 2 點需要漂移）。
- **確信度：** confirmed（程式碼的檢查範圍）／plausible（真實跨版時是否發生）。
- **來源：** 既有問題（`4518157`；空元素 `f6dac92`）。
- **建議方向：** scalar 也比 `type_name`（或至少比「整數／浮點／bool」大類）；`template_from_block` 拒絕 `mask_bytes.is_empty() && fields.is_empty()`；在 2.06 存檔上重新統計 kind 4 是否真的從不缺席，並讓空元素與 template 的規則一致；list header 改寫時依原 variant 定位 count。

### G-11 — 插入元素時不檢查 class 是否屬於目標 list

- **位置：** `src/c_abi/mod.rs:3572-3593`（`list_insert_element`）、`:2836-2873`（transplant）、`src/c_abi/element_template.rs:727-746`（template insert）。
- **問題與根因：** 三個入口都只確認元素的 class 存在於目標 schema，不比對目標 list 既有元素的 class（`resolve_object_list_element_class` 這種推斷在 G 裡已有先例，`mod.rs:3166-3240`）。把 `ItemSaveData` 插進 `_mercenaryDataList`，encoder 依元素自己的 wrapper type 寫出，re-decode 依 wrapper type 解回，所以結構上完全一致，呼叫回 `OK`；遊戲讀到的是一個 class 不對的 list 元素。
- **可重現的情境：** 以 `crimson_save_make_empty_element_bytes` 產生 `ItemSaveData` 的空殼，`crimson_save_list_insert_element` 插入任一 `MercenaryClanSaveData._mercenaryDataList`。預期回 `OK`。
- **嚴重度：** low（需要呼叫端錯誤；遊戲端後果未知）。
- **確信度：** confirmed（缺檢查）／plausible（後果）。
- **來源：** 既有問題（`f6dac92`、`501a493`、`4518157`）。

### G-12 — `write_to_file` 不是原子寫入

- **位置：** `src/c_abi/mod.rs:4193`（`std::fs::write(path_str, &bytes)`）。
- **問題與根因：** `std::fs::write` 先截斷目標檔再寫入。C# 編輯器最常見的用法是「存回原檔」，此時磁碟滿、防毒軟體鎖檔、行程被結束或斷電發生在截斷之後，使用者唯一的存檔就變成零長度或半截。沒有「寫暫存檔 → flush → rename」的流程，也沒有備份。另外這裡不檢查 `hmac_ok`（F-10 已記錄）。
- **可重現的情境：** 讓目標磁碟只剩小於存檔大小的空間，呼叫 `crimson_save_write_to_file(h, 原存檔路徑)`。預期回 `WRITE_FAILED`，原存檔已被截斷。
- **嚴重度：** low（需要 I/O 故障；後果是存檔遺失）。
- **確信度：** plausible（`std::fs::write` 的截斷語意是已知的，沒有實際重現）。
- **來源：** 既有問題（`3eff882`）。
- **建議方向：** 寫到同目錄的暫存檔、`sync_all`、再 rename 覆蓋（Windows 上 `std::fs::rename` 可取代既有檔案）。

### G-13 — `repr(C)` 結構的 layout 守門不完整（審計重點 5）

- **位置：** `src/c_abi/mod.rs:595-618`（`CrimsonSaveHandle`）、`:655-663`（`CrimsonBlockInfo`，20 B）、`:676-681`（`CrimsonPathStep`，8 B）、`:694-703`（`CrimsonScalarBatchOp`）、`:720-730`（`CrimsonScalarPresentBatchOp`）、`:745-753`（`CrimsonListRemoveBatchOp`）、`:786-799`（`CrimsonInventoryItemRecord`，文件寫 48 B）、`:1130-1137`（`CrimsonCharacterRefRecord`，文件寫 16 B）；`mod.rs` 全檔沒有 `size_of` / `align_of` / `offset_of!`。對照：`all_items.rs:249-250`、`positions.rs:197-198` 有 `const` assert。
- **問題與根因：** 計畫書要求 `repr(C)` 結構的大小與對齊要有守門並與 `c-abi-surface.md` 一致。`CrimsonItemRecord`（64/8）與 `CrimsonPositionedEntityRecord`（56/8）做到了；其餘八個沒有。三個 batch op 帶指標與 `usize`，大小隨目標平台而變（64 位元下依欄位排列推算為 40、48、32 B，含 `make_present: i32` 與 `element_idx: u32` 後的 4 B 填充），C# 端必須用 `nint` / `nuint` 才對得上，而 `c-abi-surface.md` 沒有記載任何一個。`CrimsonSaveHandle` 是不透明 handle，內含 Rust 型別，標 `repr(C)` 沒有作用，反而讓人誤以為它的佈局是 ABI 的一部分。
- **嚴重度：** low（目前欄位排列看起來合理，但沒有任何機制防止日後新增欄位時悄悄改變佈局）。
- **確信度：** confirmed。
- **來源：** 既有問題（`f1dd61c`、`d4de625`、`244e0cd`、`b1a1079`、`e5ecd10`、`88ef779`）。
- **建議方向：** 為八個結構補 `const _: () = assert!(size_of::<…>() == …)`（batch op 以 `cfg(target_pointer_width = "64")` 分開），並把大小寫進 `c-abi-surface.md`；移除 `CrimsonSaveHandle` 的 `repr(C)`。

### G-14 — 列舉 API 遇到 schema 漂移時靜默變少

- **位置：** `src/c_abi/mod.rs:991-1079`（`list_inventory_items` 以 `InventorySaveData`、`_inventorylist`、`_itemList` 與欄位型別比對）、`:1233-1266`（`list_character_refs` 以 `type_name == "CharacterKey"`）；`all_items.rs:178`（`PLAYABLE_CHARACTER_KEYS = [1, 4, 6]`，註解寫「hardcoded for 1.07」）、`:381-404`；`positions.rs:243-272, 290-318`（40 B `Transform` 佈局、class 與欄位名稱）。
- **問題與根因：** 這些 API 全部以名稱與型別比對（rustdoc 認為「a future game patch reordering fields stays correct」），重新排序確實不受影響；但改名、型別變寬（例如 `_slotNo` 從 u16 變 u32，`mod.rs:1050` 只接受 `U16`）或新增可操作角色時，紀錄會靜默變少或旗標靜默變錯，沒有錯誤碼，也沒有「認不得」的計數。live 測試只斷言 `> 100`、`> 1000`（`all_items.rs:880`、`positions.rs:726`），而且只在有 `slot107`（1.12 存檔）的機器上跑。計畫書的跨區塊要求是「要能明確失敗，不能悄悄地讀錯」。
- **嚴重度：** low（只影響顯示與篩選，寫入路徑另有索引檢查）。
- **確信度：** confirmed（程式碼）／plausible（2.06 存檔上是否已發生；本回合沒有讀存檔）。
- **來源：** 既有問題（`e5ecd10`、`88ef779`、`698e221`、`aeb2fa2`、`21e4f5d`）。

### G-15 — Transplant 不檢查兩個 handle 是否相同

- **位置：** `src/c_abi/mod.rs:2796-2810`。
- **問題與根因：** 程式同時建立 `&mut *target_handle` 與 `&*source_handle`；rustdoc（`:2777-2778`）與註解（`:2805-2807`）把「兩者不同」交給 C# wrapper 保證。一行指標比較就能把 UB 換成錯誤碼。
- **嚴重度：** nit。
- **確信度：** confirmed（缺檢查）。
- **來源：** 既有問題（`501a493`）。

### G-16 — 文件與程式碼脫節

- **位置與內容：**
  1. `docs/c-abi-surface.md:258-299` 自稱完整 catalog，實際 `crimson_save_*` 共 **47** 個匯出符號（`mod.rs` 43 個，含 `scalar_getter!` 產生的 7 個；`all_items.rs`、`positions.rs` 各 1 個；`element_template.rs` 2 個），文件只具名提到 8 個，加上以簡稱帶過的 4 個 deferred 函式共 12 個。漏掉 load / free / write / hmac（F-13 第 6 點已記），以及 7 個 scalar getter、`get_block_info`、`get_block_class_name`、`get_block_json`、`list_character_refs`、全部 scalar / typed / batch setter、list remove / clone / insert / transplant、presence toggle、dynamic array get / set、inline bytes get / set，共 35 個。
  2. `docs/save-deferred-redecode.md:88-92` 說非 deferred 的 `*_batch`「too leave earlier ops applied on a mid-batch failure」；實際上三個 batch 在非 deferred 模式都是全有或全無（`mod.rs:2376-2404` 先驗證全部；`:2575-2590`、`:3410-3431` 走 `apply_length_changing_mutation` 的 snapshot 還原）。同檔 `:52-55` 的改長度入口清單漏了 `set_object_list_present`、`transplant_list_element`、`list_insert_element_template`（三者都經由 `apply_length_changing_mutation`，確實會遵守 deferred）。
  3. `docs/save-mutation-version.md:124-125` 仍寫「It is **not** thread-safe … single-threaded by contract」，與 `7e2cf87` 之後的全域鎖不符；`:31-33` 說失敗的 mutation 一律還原（含版本），PANIC（G-05）與 deferred 中的單一操作失敗都不成立。
  4. `src/c_abi/mod.rs:3660` 「Serialize the in-memory save back to `path` using the original nonce.」是 `crimson_save_write_to_file` 的 rustdoc 首行，卻黏在 `crimson_save_dynamic_array_set_u32_elements` 上；`write_to_file` 的 rustdoc（`:4158`）因此少了第一句。
  5. `src/c_abi/mod.rs:2634-2647`：`list_clone_element` 的文件同時說「`dst_element_idx == count + 1` would be illegal」與「must be `<= count + 1`」；程式（`:2694`）要求 `<= count`。
  6. `src/c_abi/mod.rs:2902-2905`：說 scalar 已 present 再 present 是 no-op；實際會用 `init_bytes` 覆寫值（`:3011-3013`）。
  7. `src/c_abi/mod.rs:3085-3087`：`set_object_list_present` 的錯誤清單沒有 `NOT_FOUND`（`resolve_object_list_element_class` 找不到範本時回傳，`:3239`）；`:3024-3035` 的註解說 encoder「can't re-emit `marker_run_plus_zeros` with a different count」，`858cd30` 之後已支援（`:1783-1791`）。
  8. `src/c_abi/positions.rs:165` 說 `GIMMICK` 的 `element_index` 是 0、應改用 `block_idx`；程式（`:427, 452`）填的是在 `FieldSaveData._fieldGimmickSaveDataList` 中的索引，`block_idx` 是 `FieldSaveData` 而不是 gimmick 本身。`:109-118` 的 `position_kind::GIMMICK` 說是「top-level `FieldGimmickSaveData` block」，與模組文件 `:18-24` 矛盾；`:76-77` 的 yaw 公式（`2 * atan2(qy, qw)`）與實作（`:280-286`）不同（只在純 Y 軸旋轉時相等）；`:238-243` 說 `Transform` 是 InlineBytes，`:266-271` 實際讀 `ScalarValue::Bytes`。
  9. `src/c_abi/mod.rs:1491-1510`：block JSON 的格式說明漏了 `class_name`、`child`、`elements`，並說缺席欄位的 `value` 是空字串（實際是 `"(absent)"`，`:4608`）；`:4627-4631` 叫呼叫端依 JSON 的 `bytes` / `count` 做 round-trip，但 JSON 沒有輸出這兩個鍵。
  10. `src/c_abi/mod.rs:945-948`（「No allocation beyond the output buffer」）與 `:1159-1161`（「Single allocation up front」）：兩者都先收集進內部 `Vec` 再複製。
  11. `src/c_abi/mod.rs:2523-2525`、`:3328-3330`：說「On error, written with the index of the failing op」；encode 失敗（`MUTATION_INVALID`）時寫的是 `usize::MAX`（`:2594`、`:3435`）。
  12. 錯誤碼不一致：remove / clone / insert 對非 list 的欄位回 `NOT_SCALAR`（`:2488, 2620, 2688, 3585`），transplant 與 template insert 回 `NOT_OBJECT_LIST`（`:2864`、`element_template.rs:739`）。
  13. `src/c_abi/all_items.rs:659-664` 說「Flat-list every player-owned item」；實際列出全部（含 NPC 傭兵），以 `IS_PLAYER_OWNED` 旗標區分。
- **嚴重度：** nit。
- **確信度：** confirmed。
- **來源：** 既有問題。

---

## 審計重點逐條對照

| 審計重點 | 結論 |
|---|---|
| 1. FFI 安全性：`catch_unwind`、null 檢查、`from_raw_parts` 的長度來源、兩段呼叫在 buffer 不足時的回傳 | 47 個入口全部在 `catch_unwind` 內執行，或只打包參數後委派給有 `catch_unwind` 的入口（typed setter，`mod.rs:2038-2251`）。每個必填指標都在進入前檢查，`(ptr, len)` 一律「`ptr` 為 null 時 `len` 必須為 0」，切片經 `slice_from_raw_or_empty` 或 `path_len == 0` 分支建立，長度完全由呼叫端提供（只能靠契約）。兩段呼叫都回 `BUFFER_TOO_SMALL` 並填好 required / count，0 筆時回 `OK`。問題在別處：兩個列舉入口沒取鎖（G-01）、panic 沒有 rollback（G-05）、transplant 的 aliasing（G-15）。 |
| 2. 快照一致性：`mutation_version` 在改長度路徑上都遞增；deferred 期間讀取的行為；abort 後回復 | 非 deferred 的成功路徑都恰好遞增一次（in-place setter `:1921, 2006, 2424`；`apply_length_changing_mutation` `:1726`；`end_*` `:4321`），失敗路徑不遞增。`abort_*` 還原 `blocks` 與版本（`:4355-4356`），`end_*` 失敗時還原 `blocks`，`write_to_file` 在 batch 中回 `BATCH_IN_PROGRESS`，這些都與文件一致。但 batch 期間讀取 API 看到新樹、版本卻不變，兩份文件互相矛盾（G-04）；deferred 內新 present 的欄位無法設值（G-08）；PANIC 不還原（G-05）。 |
| 3. Element template 跨 schema 重建（欄位消失、新增、kind / size 改變；`meta_kind` 4 特例） | 欄位消失：丟棄並計入 `out_dropped_fields`（`element_template.rs:394-398`）。欄位新增：缺席，kind 3 / 6 / 7 帶 marker（`:565`）。kind 或 size 改變：scalar、inline、array、locator、list 五種都回 `TEMPLATE_MISMATCH`（`:421, 428, 434, 453, 460-465`），由合成測試 `refuses_what_it_cannot_carry_over` 部分釘住（scalar 寬度、scalar → list、缺 class、缺 locator 範本）。缺口：同寬度的型別變更不攔、placeholder 被接受、kind 4 特例無法分辨缺席與新增且證據只到 2.03、zero4 header 被改寫（G-10）。解析端有界：`checked_add` / `checked_mul`、u16 計數、`MAX_DEPTH = 32`、拒絕尾端多餘位元組與未知 kind。 |
| 4. Batch API 中途失敗時是全有或全無 | 非 deferred：三個 batch 都是全有或全無（scalar batch 先驗證全部；remove 與 presence batch 走 snapshot 還原）。Deferred：逐個套用，第一個失敗的之前都已生效，由呼叫端決定是否 `abort_*`（程式註解 `:2339-2344` 有寫）。`save-deferred-redecode.md:88-92` 把非 deferred 也說成部分套用，是錯的（G-16 第 2 點）；encode 失敗時 `out_failed_op_index` 是 `usize::MAX`（G-16 第 11 點）。 |
| 5. `repr(C)` 結構的大小、對齊與 `c-abi-surface.md` 一致 | `CrimsonItemRecord` 64/8、`CrimsonPositionedEntityRecord` 56/8 有 `const` assert，與文件相符；`CrimsonInventoryItemRecord` 的 48 B 只寫在文件裡；其餘七個沒有守門也沒有文件（G-13）。 |

## 跨區塊關注點

| 主題 | 結論 |
|---|---|
| 不可信輸入的配置上限 | G 自己的配置都以呼叫端給的長度或已解碼的樹為上限：`Vec::with_capacity(op_count)`（`mod.rs:2379`）、`init_bufs`（`:3396-3405`）、`with_capacity(new_elems.len() * 4)`（`:3753`）都與呼叫端的 buffer 等量；template 解析的 `with_capacity` 以 u16 為上限（`element_template.rs:285`），list 元素數不預先配置。不可信的存檔內容經由 `list_insert_element` 的呼叫端位元組進入 `decode_one_list_element_bytes`，共用 F-03 的無深度上限遞迴（構造的元素位元組可讓 stack overflow 終止行程）與 F-07 的越界 panic；`write_block_json`（`:4476-4572`）與 `walk_for_character_refs`（`:1221-1283`）也是無上限的遞迴，但深度受解碼樹限制，F-03 會先發生。 |
| Panic 面與 abort | `expect("field bounds checked above")`（`:3008, 3276, 4058`）與 `write!(String).unwrap()` 不會失敗；`walk_for_character_refs`、`format_dynamic_array`、`get_u32_elements` 的索引都有長度保護；64 位元下沒有可觸及的算術溢位。可觸發的 panic：`scalar_from_bytes` 的 `data[0]`（G-08）、上游的 F-06、F-07。panic 被攔成 `PANIC`，但 handle 狀態不還原（G-05）。Abort（`catch_unwind` 擋不住）：F-02、F-03 經 load 與 insert 路徑進入。 |
| Roundtrip 對稱性 | Scalar presence toggle 依 `classify_scalar_after_mask_toggle` 決定 `FixedSuffix` 位置，配合 encoder 的 pad 擺放（`encoder.rs:261-281`），在 marker 走訪的 block 上可往返；list presence 往返只在 marker 走訪的 parent 上成立（G-09）；template 匯出 → 匯入 → 再匯出的位元組一致只由 live 測試釘住（`element_template.rs:976-988`）；deferred 與 normal 的一致性只由兩種序列釘住（G-07、G-08）。改長度後的正確性依賴 re-decode，而 re-decode 不會失敗（G-02）。 |
| 版本漂移明確失敗 | G 層沒有把 F-09 的「解碼有猜測成分」轉成錯誤：`get_block_info` 的 `fields_decoded` 只看頂層，巢狀 placeholder 只會在 JSON 裡以 `fields: []` 出現；改長度編輯照樣在部分解碼的 list 上執行（G-06）；列舉 API 靜默變少（G-14）；template 在已知條件內會明確失敗（`TEMPLATE_MISMATCH`），但有 G-10 的例外。 |
| Feature gate | `mod c_abi` 只在 `c_abi` feature 下編譯（`src/lib.rs:92-93`）；G 使用的 save 內部 item（`decode_one_list_element_bytes`、`ABSENT_MARKER`、`FieldDef`、`TypeDef`、`absent_kind_has_marker`、`encode_scalar`、`SaveError`）都以 `#[cfg(feature = "c_abi")]` 再匯出（`src/save/mod.rs:56-65`），`python` 單獨或 bare default 都不會引用它們。G 的四個檔案沒有任何 item 級的 `allow(dead_code)`，只有三處 `clippy::too_many_arguments`。本回合沒有實際編譯四種組合。 |
| `docs/c-abi-surface.md` 一致性 | 47 個符號只列 12 個（G-16 第 1 點）；結構大小只記了三個（G-13）。 |

---

## 排除說明（查證後沒有問題的項目）

計畫書「掃描時注意到的待查事項」分配給 G 的只有一條：

- **(G) `src/c_abi/mod.rs` 有 9,345 行（正式碼約 4.8k、測試約 4.5k）：** 數字正確。全檔 9,345 行；1–4770 為正式碼（其中 77–808 屬 B 區塊的 2.06 row names、DLC merge 共用函式與 `impl_name_only_bridge!` 巨集，以及 G 的 handle 與結構定義），4771–9345 共 4,575 行為測試。依計畫書只記錄，不拆分。審計時的實際影響是：G-01 這類「某個入口漏了共同前置步驟」的問題，在 9k 行的單檔加上兩個獨立小檔的結構下不容易被看到；拆分時可順便讓所有 save 入口經過同一個 `with_handle_mut` 類的包裝。

以下是審計重點與跨區塊關注點中查證後沒有問題的部分：

- **X-01 null 與長度檢查：** 所有入口在 `catch_unwind` 前檢查 handle 與必填指標；`ptr == null && len != 0` 一律回 `NULL_ARG`；batch 的每個 op 都在套用前逐一檢查（`:2320-2337`、`:2561-2570`、`:3371-3391`）。`slice_from_raw_or_empty` 正確處理 `(null, 0)`。
- **X-02 兩段呼叫：** `get_block_class_name`、`get_block_json`、`make_empty_element_bytes`、`dynamic_array_get_u32_elements`、`get_inline_bytes_field`、`export_element_template` 與四個列舉 API 在 buffer 不足時都回 `BUFFER_TOO_SMALL` 並填好所需大小；字串類包含 NUL，位元組類不含，與各自的文件一致。
- **X-03 非 deferred batch 的原子性：** 見審計重點 4。
- **X-04 版本遞增與還原：** 見審計重點 2；`begin_*` 不遞增，巢狀 `begin_*` 回 `BATCH_IN_PROGRESS`，未開啟時 `end_*` / `abort_*` 回 `BATCH_NOT_OPEN`。
- **X-05 header count 改寫：** `update_object_list_count_in_header`（`:1766-1827`）與 `update_dynamic_array_count_in_header`（`:4101-4156`）都先檢查長度，u24 / u16 溢位回 `OUT_OF_RANGE`；`marker_run_plus_zeros` 的 count 位置（尾端往前 17 B）與 decoder 記錄的 `run + 1 + 4 + 13` 一致。
- **X-06 mask 位元：** 三個 toggle 都在改寫前檢查 `field_idx / 8 < mask_bytes.len()`；`build_empty_element_bytes` 與 `make_block` 都限制 `mbc <= 16`，與 decoder 一致；`class_index as u16` 的來源受 schema 的 u16 `type_count` 限制，不會截斷。
- **X-07 Transplant 的 type index 重對應：** `remap_block_type_indices` 遞迴改寫 `class_index`、wrapper `type_index` 與 locator `child_type_index`，以名稱查目標 schema，缺 class 時回 `TRANSPLANT_TYPE_MISSING` 且不動目標（重對應在 `apply_length_changing_mutation` 之前完成）。
- **X-08 JSON 序列化：** `write_json_string` 正確跳脫 `"`、`\`、所有 < 0x20 的控制字元；浮點數的 `NaN` / `inf` 只出現在字串值內，不會產生無效 JSON；hex 與整數欄位格式固定。
- **X-09 `CrimsonItemRecord` 與 `CrimsonPositionedEntityRecord`：** `const` assert 的大小與對齊和 rustdoc 表格、`c-abi-surface.md:263-273` 一致，`record_layout_is_stable` 另以 offset 測試釘住。
- **X-10 `write_to_file` 與 batch：** batch 開啟時回 `BATCH_IN_PROGRESS`，不會寫出過時的 body（與 F 區塊 E-10 相同結論）。
- **X-11 Template 線格式：** 讀取全部經 `Reader::take` 的 `checked_add` 與 `get`；`sized` 用 `checked_mul`；字串要求 UTF-8；`decode_template` 拒絕尾端多餘位元組；寫入端以 `try_from` 檢查 u16 / u8 / u32 上限。
- **X-12 鎖的 poison 回收本身：** `unwrap_or_else(|p| p.into_inner())` 不會讓後續呼叫卡住；問題只在回收後的 handle 狀態（G-05）。

## 未能驗證、建議在有 fixtures 與 live save 的機器上補做

| 項目 | 建議方式 | 相關 finding |
|---|---|---|
| 列舉 API 與 mutation 並行時的崩潰 | 兩個執行緒對同一 handle：一個反覆 `list_clone_element` / `list_remove_element`，一個反覆 `list_all_items` / `list_field_positions`；在 debug build 或 Application Verifier 下觀察 | G-01 |
| 跨版 transplant 是否回 `OK` 並留下解不出的元素 | 1.10 fixture 的傭兵元素 transplant 到 2.06 存檔的副本，檢查 JSON 的 `undecoded_ranges` / `<unknown>` 與回傳碼 | G-02 |
| `list_insert_element` 接受 placeholder 與尾端多餘位元組 | 構造 child 解不出的元素位元組，以及「正常元素 + 16 B 垃圾」，在存檔副本上插入 | G-02 |
| `set_object_list_present(1)` 對已 present 的 list | 在存檔副本上對有染色資料的物品呼叫，比較前後 JSON | G-03 |
| deferred 內 toggle 後設值 | 依 G-08 的序列在存檔副本上執行，並與 normal mode 比較 body | G-08 |
| panic 後的狀態 | 以構造的 body 讓 `decode_blocks` 越界，接著依 G-05 的序列操作，比較 `save.body` 前 4 KB | G-05 |
| 2.06 存檔上 kind 4 是否真的從不缺席；是否存在 count > 0 的 zero4 list | 掃描 2.04–2.06 的 live save：統計 `meta_kind == 4 && !present` 與 `header_variant == "zero4_count_u32" && count > 0` | G-10 |
| 2.06 存檔上列舉 API 的紀錄數與 1.12 基準的差異 | 對 2.06 live save 執行 `list_all_items` / `list_field_positions`，比對各 kind 的數量與 `PLAYABLE_CHARACTER_KEYS` 是否仍涵蓋所有可操作角色 | G-14 |
