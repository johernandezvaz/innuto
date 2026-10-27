# How rekordbox 7.2.18 works: evidence behind Innuto

Target: `C:\Program Files\rekordbox\rekordbox 7.2.18` (Windows x64), with the
user's own analysis data in `%APPDATA%\Pioneer\rekordbox`. Analysis used
REA 6.8.0 (`npx rea-agents@latest`) plus direct reads of files whose format is
documented below. Nothing was executed, injected, or modified in rekordbox, and
no rekordbox code was decompiled or copied into Innuto.

Labels: **Observed** = read directly from bytes; **Derived** = computed from
observed facts; **Inferred** = a reasonable interpretation that was not proven;
**Unknown** = not established.

## 1. What the install is made of

| Finding | Label | Evidence |
|---|---|---|
| 3,013 files; REA found exactly one runtime candidate: an Electron app at `rekordboxAgent-win32-x64/resources/app.asar`. The main `rekordbox.exe` (100.6 MB) is not Electron. | Observed / Inferred (family) | `inspect_desktop_application` `ev_5d9850e7…63763` (summary `ev_39c356aa…d61e`) |
| Bundled native components: `sqlite3.dll`, OpenSSL 3 (`libcrypto-3`, `libssl-3`), `tensorflow_cc.dll` (325 MB), `onnxruntime.dll` + `DirectML.dll`, `Demucs3.dll`, `Spleeter.dll`, `libmpg123.dll`, `NFSDaemon.dll`, `clatc.dll`, `lsapiw64.dll`, `zip.dll`, `zlib.dll`. | Observed (file inventory) | directory listing |
| Folders for skins (PNG/SVG/XML), `MidiMappings` (per-controller CSV), `firmware`, `drivers`, `models`, lighting/sample packs as ZIPs. | Observed | directory listing |

## 2. The main program (`rekordbox.exe`)

From its PE import table (`inspect_pe_imports`, `ev_42c19086…97cf`, SHA-256
`a99896cf…2493`): 41 import descriptors and 3 delay-load descriptors.

| Subsystem | What the imports show | Label |
|---|---|---|
| Library database | `sqlite3_open_v2`, `prepare_v2`, `step`, `backup_*`, `create_function_v2`, `load_extension`, **`sqlite3_key`** | Observed |
| Database encryption | `sqlite3_key` is a SQLCipher-style keying API, not part of stock SQLite. The shipped `master.db` has no `SQLite format 3` header, its first 4 KiB have entropy 7.95 bits/byte (`inspect_byte_entropy` `ev_e5b39be7…5341`), and REA's SQLite reader rejects it as "file is not a database". So the library is encrypted at rest. | Observed facts → **Inferred** encryption |
| Audio decoding | Media Foundation (`MFCreateSourceReaderFromURL`, `MFCreateSinkWriterFromURL`, `MFTranscodeGetAudioOutputAvailableTypes`) plus bundled `libmpg123.dll` | Observed |
| Stem separation / ML | `tensorflow_cc.dll` (C++ API: `SavedModel`, `RunOptions`, `ListAllPhysicalDevices`) and `onnxruntime.dll`; the `Demucs3.dll`/`Spleeter.dll` names match well-known stem-separation models | Observed imports; model role **Inferred** from names |
| Controllers | `WINMM` MIDI in/out (`midiInOpen`, `midiOutShortMsg`, `midiOutLongMsg`…), `HID.DLL`, `SETUPAPI` device enumeration | Observed |
| Rendering / UI | 342 `OPENGL32` functions, `d3d9`/`d3d11`/`dxgi`/`dxva2`, raw `CreateWindowExA`/`RegisterClassA`, `IMM32` text input: a custom-drawn UI, not native Win32 controls or Qt | Observed imports; "custom UI" **Inferred** |
| CDJ / USB export and network | `NFSDaemon.dll` (`StartNFSDaemon`, `SetExportListEntry`, `AddLinkMember`…), `IPHLPAPI` routing/ARP calls, `wlanapi`, `WS2_32`, `MPR` network drives | Observed; Pro DJ Link file serving to players **Inferred** from names |
| Cloud / licensing | `WININET` HTTP, OpenSSL TLS, `CRYPT32` certificate chains, `clatc.dll` with obfuscated exported class/method names | Observed; licensing purpose of `clatc` **Unknown** |
| Archives | `zip.dll` (`zip_open`, `zip_fopen_index`…) for the bundled ZIP packs | Observed |

Audio output: no WASAPI/ASIO import appears in the import table. Output is
loaded some other way (COM activation, `LoadLibrary`, or a statically linked
framework). Which one is **Unknown**.

## 3. The helper app (`rekordboxAgent`, Electron)

`inspect_electron_fuses` (`ev_98eda1ca…98dd`, 149 MB PE): `RunAsNode` enabled,
`EnableNodeOptionsEnvironmentVariable` enabled, `EnableNodeCliInspectArguments`
enabled, `EnableEmbeddedAsarIntegrityValidation` disabled, `OnlyLoadAppFromAsar`
disabled, `EnableCookieEncryption` disabled (all **Observed**). So the agent
would honor `--inspect` and `NODE_OPTIONS`, and REA reports that ASAR integrity
is not enforced (**Derived**). No runtime inspection was performed.

The static JavaScript reconstruction of `app.asar` (109 MB) did not finish.
REA stopped it with `provider_timeout` at its 300 s per-source budget. What
the agent does is **Unknown**. To retry, raise `--analysis-timeout-ms` or
select specific `--source-path` entries.

## 4. Track analysis files (`share/PIONEER/USBANLZ/**/ANLZ0000.{DAT,EXT,2EX,3EX}`)

The user's data has 202 `.DAT`, 202 `.EXT`, 202 `.2EX` and 199 `.3EX` files (**Observed**). Byte-level read of one pair:

* File header: ASCII `PMAI`, header length 28, then the total file length; all fields big-endian (**Observed**).
* Then tagged sections: 4-byte tag, header length, total length (**Observed**).
* `.DAT` sections: `PPTH`, `PVBR`, `PQTZ`, `PWAV`, `PWV2`, `PCOB` ×2. `.EXT` adds `PWV3`, `PCO2` ×2, `PQT2`, `PWV5`, `PWV4` (**Observed**).
* `PPTH` (header 16): a 4-byte byte count, then a UTF-16BE path. In this data every path was `?/<file name>`, with no directory (**Observed**).
* `PQTZ` (header 24): beat count at +20, then 8-byte entries `{u16 bar position, u16 tempo×100, u32 time ms}`. Example: 788 beats, first `{1, 12600, 226}` = 126.00 BPM at 226 ms (**Observed**).
* `PWAV` (header 20): entry count at +12 (400), constant `0x00010000` at +16, then one byte per column. The bytes look like `0x53…0x76`, which fits "low 5 bits height, high 3 bits colour/whiteness" (layout **Observed**, bit meaning **Inferred**).
* `PCOB`: two empty cue lists in this data, typed 1 and 0 (likely hot and memory cues). Cue entry layout is **Unknown** here, because no cues were present.

Innuto's `src/anlz.rs` implements exactly the observed parts and keeps every
other tag as opaque. The test `written_header_sizes_match_observed_layout`
pins Innuto's writer to the observed header sizes. Whether CDJ hardware accepts
Innuto-written files is **Unknown**: only path, grid and preview sections are
written, and no player was tested.

## 5. Controller mappings (`MidiMappings/*.midi.csv`)

* First line is `@file,1,<device>`. Header: `#name,function,type,input,deck1..deck4,output,deck1..deck4,option,comment`. Lines starting `#` are section comments (**Observed**).
* `input` is 4 hex digits: status + data1 (`900B` = note-on ch1, note 0x0B) (**Observed**).
* A deck column holds either a small number or a 4-hex code. With a number (`PlayPause … 900B,0,1`), that number is added to the status channel (deck 2 = `910B`). With a code (`Load …,,9646,9647`), that code is used as-is (**Inferred** from consistent patterns across the DDJ-400 file, 243 lines).
* `KnobSliderHiRes` controls (`TempoSlider B000`, `CrossFader B61F`) are 14-bit. The LSB is assumed to arrive on CC+0x20 (**Inferred** from the MIDI convention). Innuto uses only the 7-bit MSB.

## 6. Other user data seen (structure only)

`masterPlaylists6.xml` is plain XML (`MASTER_PLAYLIST Version="3.0.0"`, `NODE Id ParentId Attribute Timestamp Lib_Type`): a playlist tree mirror (**Observed**). The rest of the library lives in the encrypted `master.db` (**Inferred**, §2).

## 7. How Innuto relates to this

| rekordbox (observed) | Innuto design choice |
|---|---|
| Encrypted SQLite library | Plain SQLite (`library.rs`), schema versioned via `PRAGMA user_version`. No attempt is made to read rekordbox's encrypted DB. |
| PQTZ beat grid / PWAV preview | Same data model: constant-tempo grid (bpm + first beat), 400-column 5+3-bit preview |
| ANLZ files | Reads them (`innuto anlz`), imports grids by file name (`import-anlz`), writes `.DAT` (`export-anlz`) |
| CSV MIDI mappings | Parses the same CSV layout, so the shipped controller files drive Innuto's decks (`play --midi-map`) |
| Two decks + mixer | Original engine: pitch ±%, cue, beat-phase sync, constant-power crossfader |
| Stem separation, lighting, cloud, Pro DJ Link, recording, FX, key detection | **Not implemented** |

## 8. Measured analysis accuracy versus rekordbox

On the two full-length tracks with both a rekordbox grid and a local audio file
(rekordbox's bundled demo/sample content):

| rekordbox | Innuto | Phase difference |
|---|---|---|
| 120.00 BPM, first beat 13 ms | 120.00 BPM, 29 ms | 16 ms of a 500 ms beat |
| 128.00 BPM, first beat 13 ms | 128.00 BPM, 433 ms | 49 ms of a 469 ms beat |

Tempo matched exactly on both. Phase is close but not exact. On the 128 BPM
track, Innuto places the beat on a broadband low-end hit about 50 ms before
rekordbox's beat. An ffmpeg low-pass fold of that track puts the main kick
energy just after rekordbox's 13 ms. That is two tracks, not a benchmark.
Downbeat (bar position 1) detection is not implemented: Innuto numbers bars
from the first detected beat. The other 18 matched files were 0–9 s samples
with empty rekordbox grids, so they were left out of the comparison.
