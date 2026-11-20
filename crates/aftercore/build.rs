//! Build script: draws the logo, packs it into icon resources together with the
//! application manifest and writes a `.res` file that is passed straight to the linker.
//! No rc.exe / Windows SDK tools are required.

#[path = "../ac-overlay/src/logo.rs"]
#[allow(dead_code)]
mod logo;

use std::io::Write;
use std::path::PathBuf;

const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="AfterCore.App" version="1.0.0.0" processorArchitecture="amd64"/>
  <description>AfterCore</description>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
      <activeCodePage xmlns="http://schemas.microsoft.com/SMI/2019/WindowsSettings">UTF-8</activeCodePage>
    </windowsSettings>
  </application>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0"
        processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
</assembly>
"#;

fn res_entry(out: &mut Vec<u8>, ty: u16, name: u16, flags: u16, data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_le_bytes()); // DataSize
    out.extend_from_slice(&32u32.to_le_bytes()); // HeaderSize
    out.extend_from_slice(&0xFFFFu16.to_le_bytes());
    out.extend_from_slice(&ty.to_le_bytes());
    out.extend_from_slice(&0xFFFFu16.to_le_bytes());
    out.extend_from_slice(&name.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // DataVersion
    out.extend_from_slice(&flags.to_le_bytes()); // MemoryFlags
    out.extend_from_slice(&0x0409u16.to_le_bytes()); // LanguageId
    out.extend_from_slice(&0u32.to_le_bytes()); // Version
    out.extend_from_slice(&0u32.to_le_bytes()); // Characteristics
    out.extend_from_slice(data);
    while out.len() % 4 != 0 {
        out.push(0);
    }
}

/// One VS_VERSIONINFO node: wLength, wValueLength, wType, key, padding, value, padding, children.
fn ver_block(key: &str, value: &[u8], value_len: u16, text: bool, children: &[Vec<u8>]) -> Vec<u8> {
    let mut b = vec![0u8; 6];
    for c in key.encode_utf16().chain(std::iter::once(0)) {
        b.extend_from_slice(&c.to_le_bytes());
    }
    while b.len() % 4 != 0 {
        b.push(0);
    }
    b.extend_from_slice(value);
    for c in children {
        while b.len() % 4 != 0 {
            b.push(0);
        }
        b.extend_from_slice(c);
    }
    let len = b.len() as u16;
    b[0..2].copy_from_slice(&len.to_le_bytes());
    b[2..4].copy_from_slice(&value_len.to_le_bytes());
    b[4..6].copy_from_slice(&(text as u16).to_le_bytes());
    b
}

fn ver_string(key: &str, val: &str) -> Vec<u8> {
    let mut v = Vec::new();
    let w: Vec<u16> = val.encode_utf16().chain(std::iter::once(0)).collect();
    for c in &w {
        v.extend_from_slice(&c.to_le_bytes());
    }
    ver_block(key, &v, w.len() as u16, true, &[])
}

fn version_info() -> Vec<u8> {
    let ver = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "1.0.0".into());
    let p: Vec<u32> = ver.split('.').map(|x| x.parse().unwrap_or(0)).chain([0, 0, 0]).take(4).collect();
    let ms = (p[0] << 16) | p[1];
    let ls = (p[2] << 16) | p[3];
    let mut fixed = Vec::new();
    for v in [0xFEEF04BDu32, 0x0001_0000, ms, ls, ms, ls, 0x3F, 0, 0x0004_0004, 1, 0, 0, 0] {
        fixed.extend_from_slice(&v.to_le_bytes());
    }
    let strings = [
        ("CompanyName", "AfterCore"),
        ("FileDescription", "AfterCore - FPS counter and hardware monitor (no injection)"),
        ("FileVersion", ver.as_str()),
        ("InternalName", "AfterCore"),
        ("LegalCopyright", "© 2026 AfterCore"),
        ("OriginalFilename", "AfterCore.exe"),
        ("ProductName", "AfterCore"),
        ("ProductVersion", ver.as_str()),
    ];
    let table: Vec<Vec<u8>> = strings.iter().map(|(k, v)| ver_string(k, v)).collect();
    let string_table = ver_block("040904B0", &[], 0, true, &table);
    let sfi = ver_block("StringFileInfo", &[], 0, true, &[string_table]);
    let var = ver_block("Translation", &[0x09, 0x04, 0xB0, 0x04], 4, false, &[]);
    let vfi = ver_block("VarFileInfo", &[], 0, true, &[var]);
    ver_block("VS_VERSION_INFO", &fixed, 52, false, &[sfi, vfi])
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../ac-overlay/src/logo.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    let sizes = [16u32, 20, 24, 32, 40, 48, 64, 128, 256];
    let pngs: Vec<Vec<u8>> = sizes.iter().map(|&s| logo::logo_pixmap(s).encode_png().expect("png")).collect();

    // Standalone .ico (handy for shortcuts / docs).
    let mut ico = Vec::new();
    ico.extend_from_slice(&[0, 0, 1, 0]);
    ico.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (s, png) in sizes.iter().zip(&pngs) {
        let b = if *s >= 256 { 0 } else { *s as u8 };
        ico.extend_from_slice(&[b, b, 0, 0]);
        ico.extend_from_slice(&1u16.to_le_bytes());
        ico.extend_from_slice(&32u16.to_le_bytes());
        ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for png in &pngs {
        ico.extend_from_slice(png);
    }
    std::fs::write(out_dir.join("AfterCore.ico"), &ico).unwrap();

    // .res: empty header, RT_ICON × n, RT_GROUP_ICON, RT_MANIFEST.
    let mut res = Vec::new();
    res_entry(&mut res, 0, 0, 0, &[]);
    for (i, png) in pngs.iter().enumerate() {
        res_entry(&mut res, 3, (i + 1) as u16, 0x1010, png);
    }
    let mut grp = Vec::new();
    grp.extend_from_slice(&[0, 0, 1, 0]);
    grp.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    for (i, (s, png)) in sizes.iter().zip(&pngs).enumerate() {
        let b = if *s >= 256 { 0 } else { *s as u8 };
        grp.extend_from_slice(&[b, b, 0, 0]);
        grp.extend_from_slice(&1u16.to_le_bytes());
        grp.extend_from_slice(&32u16.to_le_bytes());
        grp.extend_from_slice(&(png.len() as u32).to_le_bytes());
        grp.extend_from_slice(&((i + 1) as u16).to_le_bytes());
    }
    res_entry(&mut res, 14, 1, 0x1030, &grp);
    res_entry(&mut res, 24, 1, 0x1030, MANIFEST.as_bytes());
    res_entry(&mut res, 16, 1, 0x0030, &version_info());

    let res_path = out_dir.join("aftercore.res");
    std::fs::File::create(&res_path).unwrap().write_all(&res).unwrap();
    println!("cargo:rustc-link-arg-bins={}", res_path.display());
}
