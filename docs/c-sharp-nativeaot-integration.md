# Bundling crimson_rs into a C# NativeAOT single-file exe

How to ship a downstream C# save editor (e.g. CrimsonAtomtic) as **one self-contained .exe** with no separate `crimson_rs.dll` next to it. Uses .NET NativeAOT's `DirectPInvoke` + `NativeLibrary` mechanism to statically link the Rust code into the AOT output.

## Prerequisites

- **.NET 8+ SDK** (NativeAOT is GA in .NET 8; earlier previews exist in .NET 7 but the MSBuild item names were unstable).
- A C# project already using `<PublishAot>true</PublishAot>` and producing a single-file native exe (`dotnet publish -r win-x64 -c Release`).
- The MSVC build toolchain (`x86_64-pc-windows-msvc`) — required for `.lib` ABI compatibility with NativeAOT's linker. Don't mix MinGW with NativeAOT.
- A 64-bit Windows host (other platforms work too, just file extensions differ — `libcrimson_rs.a` on Linux / macOS).

The Rust side is already set up:
- `Cargo.toml` declares both `cdylib` and `staticlib` so a single `cargo build --release --features c_abi` produces both artifacts in one pass.
- `.cargo/config.toml` statically links the MSVC C runtime (`+crt-static`), so the resulting `.lib` is fully self-contained — no `VCRUNTIME140.dll` or `python3.dll` references to satisfy at the linker stage.

## Step 1 — Build the static library

From the crimson-rs checkout:

```powershell
cargo build --release --features c_abi
```

The relevant artifact is `target\release\crimson_rs.lib` (~15 MB; ~5–8 MB after NativeAOT dedup against the runtime). The companion `crimson_rs.dll` (~1.2 MB) is also produced; ignore it for the AOT path.

> **Do not enable the `python` feature** for the AOT path. It pulls in PyO3 and the link-time symbol surface that PyO3 wants resolved against a host Python interpreter, which an AOT exe has no way to satisfy. The `c_abi` feature is the right one — it's pure `extern "C"`.

## Step 2 — Wire the .lib into the C# project

Drop the static library somewhere your `.csproj` can reference (e.g. `lib\crimson_rs.lib` next to the project file) and add to the `.csproj`:

```xml
<ItemGroup>
  <!-- Tell NativeAOT that any [DllImport("crimson_rs")] in our C#
       code should resolve to a direct, statically-linked call, not a
       runtime LoadLibrary. Logical name must match the [DllImport]
       string. -->
  <DirectPInvoke Include="crimson_rs" />

  <!-- Hand the linker the actual .lib file. The path is relative to
       the .csproj. -->
  <NativeLibrary Include="lib\crimson_rs.lib" />
</ItemGroup>
```

That's the entire build-side setup. The C# call sites stay identical to the existing `crimson_rs.dll` LoadLibrary path:

```csharp
[DllImport("crimson_rs", CallingConvention = CallingConvention.Cdecl)]
private static extern int crimson_save_load_from_file(
    [MarshalAs(UnmanagedType.LPUTF8Str)] string path,
    out IntPtr handle);

// ... usage exactly as before ...
```

Under NativeAOT with `<DirectPInvoke>crimson_rs</DirectPInvoke>`, the linker resolves `crimson_save_load_from_file` to the symbol inside `crimson_rs.lib` at AOT compile time. No dll is loaded at runtime; the symbol is just a direct call.

## Step 3 — Publish

```powershell
dotnet publish -r win-x64 -c Release
```

Output is a single self-contained `.exe`. Verify:

```powershell
# Should show NO crimson_rs.dll in the publish directory.
ls bin\Release\net8.0\win-x64\publish\

# Should show no import for crimson_rs.dll.
dumpbin /imports bin\Release\net8.0\win-x64\publish\YourApp.exe | findstr crimson
```

If `dumpbin` shows `crimson_rs.dll` as an import, the `<DirectPInvoke>` item wasn't picked up — most commonly because the `Include=` name doesn't match the `[DllImport(...)]` string exactly (case-sensitive on Linux; case-insensitive on Windows but match the source for hygiene).

## Trade-offs vs the current LoadLibrary-dll path

| | Static link (NativeAOT) | LoadLibrary `crimson_rs.dll` (current) |
|---|---|---|
| Files at runtime | 1 (`.exe`) | 2 (`.exe` + `.dll`) |
| Final exe size | +5–8 MB vs the dll path | baseline |
| Startup cost | Faster (no dll resolution) | One `LoadLibrary` per process |
| Update granularity | Re-publish whole exe to update Rust code | Drop in a new `.dll` |
| Build complexity | Needs cargo + dotnet publish in CI | Just cargo + copy the dll |
| Debugging | Symbols folded into the AOT exe `.pdb` | Separate `crimson_rs.pdb` |

The static-link path is mainly worth it when shipping to end users who don't want to think about "where do I put the dll" (typical for a save editor). For dev / internal builds, sticking with the dll path is usually faster to iterate on.

## When you can't use NativeAOT

Plain self-contained .NET publishing (`<PublishSingleFile>true</PublishSingleFile>` without `<PublishAot>`) bundles managed assemblies, but native dependencies are extracted to disk at first run by default. To avoid the extraction, add to the `.csproj`:

```xml
<PropertyGroup>
  <IncludeNativeLibrariesForSelfExtract>true</IncludeNativeLibrariesForSelfExtract>
</PropertyGroup>
```

This still produces a single `.exe`, but at first run it unpacks `crimson_rs.dll` into a temp directory (`%LOCALAPPDATA%\Temp\.net\YourApp\…`) and `LoadLibrary`-loads it from there. Functionally indistinguishable from "one exe" to the user, with no AOT build complexity, but with a per-process first-run extraction cost. For most save-editor use cases this is the easier path; NativeAOT is the better choice if startup time or true "zero filesystem footprint outside the exe" matter.

## Things that already make this easy

- `c_abi` feature is independent of `python` — no PyO3 contamination of the static lib.
- `.cargo/config.toml` already pins static-CRT for the MSVC target, so the static lib has no `VCRUNTIME140.dll` import for the NativeAOT linker to resolve.
- All `extern "C"` surfaces use `#[unsafe(no_mangle)]` so the C# `[DllImport]` strings match the symbol table directly.
- The CI build (see `.github/workflows/build.yml`) can be extended to upload `crimson_rs.lib` as a release artifact next to `crimson_rs.dll` whenever downstream needs a fresh static lib.
