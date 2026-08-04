use crate::model::{normalize_extension, Category};

pub fn classify(
    extension: &str,
    content_type: Option<&str>,
    perceived_type: Option<&str>,
    prog_id: Option<&str>,
    friendly_name: Option<&str>,
) -> Category {
    let ext = normalize_extension(extension);
    let bare = ext.trim_start_matches('.');

    // .ts is both MPEG transport stream and TypeScript. Let association metadata
    // decide; absent a media hint, prefer source code because that is safer for a
    // text-only icon classifier.
    if bare == "ts" {
        let hints = format!(
            "{} {} {} {}",
            content_type.unwrap_or_default(),
            perceived_type.unwrap_or_default(),
            prog_id.unwrap_or_default(),
            friendly_name.unwrap_or_default(),
        )
        .to_ascii_lowercase();
        if hints.contains("video") || hints.contains("mpeg") || hints.contains("transport stream") {
            return Category::Video;
        }
        return Category::Code;
    }

    if let Some(category) = category_for_known_extension(bare) {
        return category;
    }

    let mime = content_type.unwrap_or_default().to_ascii_lowercase();
    if mime.starts_with("text/") {
        return Category::Text;
    }
    if mime.starts_with("image/") {
        return Category::Image;
    }
    if mime.starts_with("video/") {
        return Category::Video;
    }
    if mime.starts_with("audio/") {
        return Category::Audio;
    }
    if mime.contains("pdf")
        || mime.contains("word")
        || mime.contains("opendocument.text")
        || mime.contains("rtf")
    {
        return Category::Document;
    }
    if mime.contains("spreadsheet") || mime.contains("excel") || mime.contains("csv") {
        return Category::Spreadsheet;
    }
    if mime.contains("presentation") || mime.contains("powerpoint") {
        return Category::Presentation;
    }
    if mime.contains("zip")
        || mime.contains("compressed")
        || mime.contains("archive")
        || mime.contains("tar")
    {
        return Category::Archive;
    }
    if mime.contains("sqlite") || mime.contains("database") || mime.contains("dbase") {
        return Category::Database;
    }
    if mime.contains("font") {
        return Category::Font;
    }

    let perceived = perceived_type.unwrap_or_default().to_ascii_lowercase();
    match perceived.as_str() {
        "text" => return Category::Text,
        "image" => return Category::Image,
        "video" => return Category::Video,
        "audio" => return Category::Audio,
        "compressed" => return Category::Archive,
        "document" => return Category::Document,
        "system" => return Category::System,
        _ => {}
    }

    let hints = format!(
        "{} {}",
        prog_id.unwrap_or_default(),
        friendly_name.unwrap_or_default()
    )
    .to_ascii_lowercase();

    for (needle, category) in [
        ("database", Category::Database),
        ("sqlite", Category::Database),
        ("spreadsheet", Category::Spreadsheet),
        ("worksheet", Category::Spreadsheet),
        ("presentation", Category::Presentation),
        ("slide", Category::Presentation),
        ("document", Category::Document),
        ("archive", Category::Archive),
        ("compressed", Category::Archive),
        ("source", Category::Code),
        ("script", Category::Code),
        ("project", Category::Internal),
        ("cache", Category::Internal),
        ("settings", Category::Internal),
        ("configuration", Category::Internal),
        ("font", Category::Font),
        ("model", Category::Model3d),
    ] {
        if hints.contains(needle) {
            return category;
        }
    }

    Category::Internal
}

fn category_for_known_extension(ext: &str) -> Option<Category> {
    let category = match ext {
        // Plain text and lightweight markup.
        "txt" | "text" | "log" | "nfo" | "readme" | "md" | "markdown" | "mdown" | "mkd" | "rst"
        | "adoc" | "asciidoc" | "org" | "tex" | "bib" | "ini" | "cfg" | "conf" | "properties"
        | "env" | "toml" | "yaml" | "yml" | "json" | "jsonl" | "xml" | "xsd" | "xsl" | "xslt" => {
            Category::Text
        }

        // General documents and e-books.
        "pdf" | "rtf" | "doc" | "docx" | "docm" | "dot" | "dotx" | "odt" | "ott" | "pages"
        | "wpd" | "wps" | "epub" | "mobi" | "azw" | "azw3" | "djvu" | "djv" | "xps" | "oxps"
        | "chm" | "lit" => Category::Document,

        // Spreadsheets and tabular workbooks.
        "csv" | "tsv" | "tab" | "xls" | "xlsx" | "xlsm" | "xlsb" | "xlt" | "xltx" | "ods"
        | "ots" | "numbers" | "fods" | "dif" | "slk" => Category::Spreadsheet,

        // Presentations.
        "ppt" | "pptx" | "pptm" | "pps" | "ppsx" | "pot" | "potx" | "odp" | "otp" | "key" => {
            Category::Presentation
        }

        // Raster/vector/raw images and design images.
        "jpg" | "jpeg" | "jpe" | "png" | "gif" | "bmp" | "dib" | "tif" | "tiff" | "webp"
        | "avif" | "heic" | "heif" | "jxl" | "jp2" | "j2k" | "jpf" | "svg" | "svgz" | "ico"
        | "cur" | "psd" | "xcf" | "kra" | "ai" | "eps" | "raw" | "dng" | "cr2" | "cr3" | "nef"
        | "nrw" | "arw" | "orf" | "rw2" | "raf" | "pef" | "srw" | "x3f" => Category::Image,

        // Video and container formats primarily used for video.
        "mp4" | "m4v" | "mkv" | "webm" | "avi" | "mov" | "qt" | "wmv" | "asf" | "flv" | "f4v"
        | "mpeg" | "mpg" | "mpe" | "m2v" | "mts" | "m2ts" | "vob" | "ogv" | "3gp" | "3g2"
        | "rm" | "rmvb" => Category::Video,

        // Audio and playlists.
        "mp3" | "wav" | "flac" | "aac" | "m4a" | "ogg" | "oga" | "opus" | "wma" | "aiff"
        | "aif" | "ape" | "alac" | "mid" | "midi" | "amr" | "m3u" | "m3u8" | "pls" | "cue" => {
            Category::Audio
        }

        // Archives, disk images, packages, and compressed streams.
        "zip" | "7z" | "rar" | "tar" | "gz" | "gzip" | "bz2" | "bzip2" | "xz" | "zst" | "lz"
        | "lzma" | "cab" | "arj" | "ace" | "tgz" | "tbz" | "tbz2" | "txz" | "iso" | "img"
        | "vhd" | "vhdx" | "wim" | "dmg" | "pkg" | "deb" | "rpm" | "apk" | "jar" | "war"
        | "ear" => Category::Archive,

        // Databases, indexes, stores, and database journals.
        "db" | "db3" | "sqlite" | "sqlite3" | "sqlite2" | "mdb" | "accdb" | "dbf" | "sql"
        | "bak" | "dump" | "parquet" | "feather" | "arrow" | "orc" | "duckdb" | "ldb" | "sdf"
        | "mdf" | "ndf" | "ldf" | "wal" | "shm" | "journal" | "idx" | "index" => Category::Database,

        // Programming, build, shader, and web source.
        "rs" | "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "cs" | "java" | "kt" | "kts" | "go"
        | "py" | "pyw" | "pyi" | "rb" | "php" | "swift" | "m" | "mm" | "scala" | "lua" | "pl"
        | "pm" | "r" | "jl" | "dart" | "ex" | "exs" | "erl" | "hrl" | "fs" | "fsx" | "vb"
        | "vbs" | "ps1" | "psm1" | "sh" | "bash" | "zsh" | "fish" | "bat" | "cmd" | "js"
        | "mjs" | "cjs" | "jsx" | "tsx" | "html" | "htm" | "css" | "scss" | "sass" | "less"
        | "vue" | "svelte" | "wasm" | "wat" | "asm" | "s" | "glsl" | "hlsl" | "vert" | "frag"
        | "comp" | "cmake" | "gradle" | "make" | "mk" | "ninja" | "sln" | "vcxproj" | "csproj"
        | "fsproj" | "vbproj" | "xcodeproj" => Category::Code,

        // System binaries, executable formats, shortcuts, and installer metadata.
        "exe" | "com" | "dll" | "sys" | "drv" | "ocx" | "cpl" | "scr" | "efi" | "msi" | "msp"
        | "mst" | "msix" | "msixbundle" | "appx" | "appxbundle" | "lnk" | "url" | "reg" | "msc"
        | "manifest" | "cat" | "inf" | "mui" => Category::System,

        // 3D/CAD/model formats.
        "obj" | "fbx" | "gltf" | "glb" | "stl" | "3ds" | "dae" | "blend" | "ply" | "step"
        | "stp" | "iges" | "igs" | "dwg" | "dxf" | "skp" | "usd" | "usda" | "usdc" | "usdz"
        | "3mf" => Category::Model3d,

        // Fonts.
        "ttf" | "otf" | "ttc" | "woff" | "woff2" | "eot" | "pfb" | "pfm" | "fnt" | "fon" => {
            Category::Font
        }

        // Common app-internal/project/cache formats.
        "dat" | "bin" | "cache" | "tmp" | "temp" | "lock" | "pid" | "state" | "session"
        | "workspace" | "project" | "proj" | "user" | "prefs" | "settings" | "sav" | "save"
        | "pak" | "assets" | "asset" | "res" | "resource" | "blob" | "store" | "meta" => {
            Category::Internal
        }

        _ => return None,
    };
    Some(category)
}

pub fn is_protected_extension(extension: &str) -> bool {
    matches!(
        normalize_extension(extension).as_str(),
        ".exe"
            | ".com"
            | ".bat"
            | ".cmd"
            | ".dll"
            | ".sys"
            | ".drv"
            | ".ocx"
            | ".cpl"
            | ".scr"
            | ".efi"
            | ".msi"
            | ".msp"
            | ".mst"
            | ".msix"
            | ".msixbundle"
            | ".appx"
            | ".appxbundle"
            | ".lnk"
            | ".url"
            | ".reg"
            | ".msc"
            | ".manifest"
            | ".cat"
            | ".inf"
            | ".mui"
    )
}

pub fn known_label(extension: &str) -> Option<&'static str> {
    match normalize_extension(extension).as_str() {
        ".jpeg" | ".jpe" => Some("JPG"),
        ".tiff" => Some("TIF"),
        ".markdown" | ".mdown" => Some("MD"),
        ".asciidoc" => Some("ADOC"),
        ".sqlite" | ".sqlite2" | ".sqlite3" => Some("SQLT"),
        ".javascript" => Some("JS"),
        ".typescript" => Some("TS"),
        ".powershell" => Some("PS1"),
        ".bzip2" => Some("BZ2"),
        ".gzip" => Some("GZ"),
        ".msixbundle" => Some("MSXB"),
        ".appxbundle" => Some("APXB"),
        _ => None,
    }
}

pub fn make_label(extension: &str, max_chars: usize) -> String {
    if let Some(label) = known_label(extension) {
        return label.chars().take(max_chars.max(1)).collect();
    }

    let raw: Vec<char> = normalize_extension(extension)
        .trim_start_matches('.')
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .flat_map(|ch| ch.to_uppercase())
        .collect();

    if raw.is_empty() {
        return "FILE".chars().take(max_chars.max(1)).collect();
    }

    let limit = max_chars.max(1);
    if raw.len() <= limit {
        return raw.iter().collect();
    }

    let mut chosen: Vec<usize> = vec![0];

    // Digits and consonants usually carry more extension identity than vowels.
    for (index, ch) in raw.iter().enumerate().skip(1) {
        if chosen.len() >= limit {
            break;
        }
        if ch.is_ascii_digit() || (ch.is_ascii_alphabetic() && !is_ascii_vowel(*ch)) {
            chosen.push(index);
        }
    }

    for index in 1..raw.len() {
        if chosen.len() >= limit {
            break;
        }
        if !chosen.contains(&index) {
            chosen.push(index);
        }
    }

    chosen.sort_unstable();
    chosen.into_iter().map(|index| raw[index]).collect()
}

fn is_ascii_vowel(ch: char) -> bool {
    matches!(ch, 'A' | 'E' | 'I' | 'O' | 'U')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_short_and_deterministic() {
        assert_eq!(make_label(".docx", 4), "DOCX");
        assert_eq!(make_label("sqlite", 4), "SQLT");
        assert_eq!(make_label("jsonlines", 4), "JSNL");
        assert_eq!(make_label(".jpeg", 4), "JPG");
    }

    #[test]
    fn known_categories_win() {
        assert_eq!(classify(".pdf", None, None, None, None), Category::Document);
        assert_eq!(classify(".rs", None, None, None, None), Category::Code);
    }
}
