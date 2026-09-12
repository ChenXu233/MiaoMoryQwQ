fn main() {
    tauri_build::build();
    remove_ort_sidecar_dlls();
}

// ort-sys（directml/cuda feature）构建期从 pyke CDN 下载 EP 供给库，并把
// DirectML.dll 以符号链接放进 <profile> 目录（链接指向 %LOCALAPPDATA% 的
// ort.pyke.io 下载缓存）。CDN 下载失败时链接悬空，而 tauri 打包会自动收录
// 主二进制旁的 *.dll → WiX light 打开悬空链接直接失败（LGHT0001，
// "Could not find a part of the path"），且 CDN 时好时坏导致 CI 间歇性红。
// 本应用的 EP 变体完全由 runtime/dml 部署负责（ADR-0014），pyke 产物一律移除，
// 保证本地与 CI 产物一致。
fn remove_ort_sidecar_dlls() {
    let Ok(out_dir) = std::env::var("OUT_DIR") else {
        return;
    };
    // OUT_DIR = <target>/<profile>/build/<pkg>-<hash>/out → 上溯三级到 <profile>
    let Some(profile_dir) = std::path::Path::new(&out_dir).ancestors().nth(3) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(profile_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_dll = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("dll"));
        if !is_dll {
            continue;
        }
        // DirectML.dll 无条件移除；其余 DLL 仅在悬空（metadata 失败 = 目标不可达）时移除
        let is_directml = path.file_name().is_some_and(|n| n.eq_ignore_ascii_case("DirectML.dll"));
        let dangling = std::fs::metadata(&path).is_err();
        if is_directml || dangling {
            let _ = std::fs::remove_file(&path);
            println!("cargo:warning=已移除 ort-sys 侧车 DLL：{}", path.display());
        }
    }
}
