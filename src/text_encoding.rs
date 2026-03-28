//! 工作区文本文件编码：显式声明或 BOM/嗅探，解码遇非法序列时返回明确错误（不做有损替换静默乱码）。
//!
//! 供 `read_file`、`extract_in_file`、`GET /workspace/file` 等复用。

use std::fs::File;
use std::io::Read;
use std::path::Path;

use chardetng::EncodingDetector;
use encoding_rs::{BIG5, DecoderResult, Encoding, GB18030, GBK, UTF_8, UTF_16BE, UTF_16LE};

/// 读取文件头用于 BOM/自动嗅探的最大字节数。
pub const SNIFF_MAX_BYTES: usize = 64 * 1024;
const READ_CHUNK: usize = 32 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEncodingName {
    Utf8,
    Utf8Sig,
    Gb18030,
    Gbk,
    Big5,
    Utf16Le,
    Utf16Be,
    Auto,
}

#[derive(Debug, Clone, Copy)]
pub enum ResolvedTextEncoding {
    /// 按 UTF-8 严格校验（`BufRead::read_line`）；非法序列会报 `InvalidData`。
    Utf8Strict,
    /// 跳过 UTF-8 BOM（若存在）后按 UTF-8 读行。
    Utf8Sig { skip_bom: usize },
    /// 用 `encoding_rs` 流式解码；遇非法序列立即报错。
    Decoder {
        encoding: &'static Encoding,
        label: &'static str,
    },
}

#[derive(Debug, Clone)]
pub struct DecodedFileNote {
    pub label: &'static str,
    pub auto_detected: bool,
}

/// 解析工具 / API 传入的 `encoding` 字符串（大小写不敏感，`_` 与 `-` 等价）。
pub fn parse_text_encoding_name(raw: Option<&str>) -> Result<TextEncodingName, String> {
    let s = raw.unwrap_or("utf-8").trim();
    if s.is_empty() {
        return Ok(TextEncodingName::Utf8);
    }
    let n = s.to_ascii_lowercase().replace('_', "-");
    match n.as_str() {
        "utf-8" | "utf8" => Ok(TextEncodingName::Utf8),
        "utf-8-sig" | "utf8-sig" | "utf8sig" => Ok(TextEncodingName::Utf8Sig),
        "gb18030" => Ok(TextEncodingName::Gb18030),
        "gbk" | "gb2312" => Ok(TextEncodingName::Gbk),
        "big5" | "big5-hkscs" | "big5hkscs" => Ok(TextEncodingName::Big5),
        "utf-16le" | "utf16le" => Ok(TextEncodingName::Utf16Le),
        "utf-16be" | "utf16be" => Ok(TextEncodingName::Utf16Be),
        "auto" => Ok(TextEncodingName::Auto),
        _ => Err(format!(
            "错误：不支持的 encoding「{}」。可选：utf-8、utf-8-sig、gb18030、gbk、gb2312、big5、utf-16le、utf-16be、auto",
            s
        )),
    }
}

fn bom_encoding_and_skip(bytes: &[u8]) -> Option<(&'static Encoding, usize)> {
    if bytes.len() >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF {
        return Some((UTF_8, 3));
    }
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        return Some((UTF_16LE, 2));
    }
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        return Some((UTF_16BE, 2));
    }
    None
}

fn static_label(enc: &'static Encoding) -> &'static str {
    if enc == UTF_8 {
        "UTF-8"
    } else if enc == UTF_16LE {
        "UTF-16LE"
    } else if enc == UTF_16BE {
        "UTF-16BE"
    } else if enc == GB18030 {
        "GB18030"
    } else if enc == GBK {
        "GBK"
    } else if enc == BIG5 {
        "Big5"
    } else {
        enc.name()
    }
}

/// 根据文件头与调用方意图确定解码方式；`skip_bom` 为从**文件首字节**起应跳过的 BOM 长度。
pub fn resolve_text_encoding(
    head: &[u8],
    hint: TextEncodingName,
) -> Result<(ResolvedTextEncoding, DecodedFileNote), String> {
    let bom = bom_encoding_and_skip(head);

    match hint {
        TextEncodingName::Utf8 => {
            if let Some((enc, _skip)) = bom
                && enc == UTF_8
            {
                return Ok((
                    ResolvedTextEncoding::Utf8Strict,
                    DecodedFileNote {
                        label: "UTF-8",
                        auto_detected: false,
                    },
                ));
            }
            Ok((
                ResolvedTextEncoding::Utf8Strict,
                DecodedFileNote {
                    label: "UTF-8",
                    auto_detected: false,
                },
            ))
        }
        TextEncodingName::Utf8Sig => {
            let skip_bom = bom
                .filter(|(e, _)| *e == UTF_8)
                .map(|(_, s)| s)
                .unwrap_or(0);
            Ok((
                ResolvedTextEncoding::Utf8Sig { skip_bom },
                DecodedFileNote {
                    label: "UTF-8（去 BOM）",
                    auto_detected: false,
                },
            ))
        }
        TextEncodingName::Gb18030 => Ok((
            ResolvedTextEncoding::Decoder {
                encoding: GB18030,
                label: "GB18030",
            },
            DecodedFileNote {
                label: "GB18030",
                auto_detected: false,
            },
        )),
        TextEncodingName::Gbk => Ok((
            ResolvedTextEncoding::Decoder {
                encoding: GBK,
                label: "GBK",
            },
            DecodedFileNote {
                label: "GBK",
                auto_detected: false,
            },
        )),
        TextEncodingName::Big5 => Ok((
            ResolvedTextEncoding::Decoder {
                encoding: BIG5,
                label: "Big5",
            },
            DecodedFileNote {
                label: "Big5",
                auto_detected: false,
            },
        )),
        TextEncodingName::Utf16Le => Ok((
            ResolvedTextEncoding::Decoder {
                encoding: UTF_16LE,
                label: "UTF-16LE",
            },
            DecodedFileNote {
                label: "UTF-16LE",
                auto_detected: false,
            },
        )),
        TextEncodingName::Utf16Be => Ok((
            ResolvedTextEncoding::Decoder {
                encoding: UTF_16BE,
                label: "UTF-16BE",
            },
            DecodedFileNote {
                label: "UTF-16BE",
                auto_detected: false,
            },
        )),
        TextEncodingName::Auto => {
            if let Some((enc, _skip)) = bom {
                let label = static_label(enc);
                return Ok((
                    ResolvedTextEncoding::Decoder {
                        encoding: enc,
                        label,
                    },
                    DecodedFileNote {
                        label,
                        auto_detected: false,
                    },
                ));
            }
            let mut det = EncodingDetector::new();
            det.feed(head, true);
            let enc = det.guess(None, true);
            let label = static_label(enc);
            Ok((
                ResolvedTextEncoding::Decoder {
                    encoding: enc,
                    label,
                },
                DecodedFileNote {
                    label,
                    auto_detected: true,
                },
            ))
        }
    }
}

/// 打开文件并读取不超过 `max_head` 的前缀（用于嗅探）；文件指针位于已读字节之后。
pub fn open_file_and_read_head(path: &Path, max_head: usize) -> Result<(File, Vec<u8>), String> {
    let mut file = File::open(path).map_err(|e| format!("打开文件失败: {}", e))?;
    let len = file
        .metadata()
        .map_err(|e| format!("读取元数据失败: {}", e))?
        .len() as usize;
    let take = max_head.min(len);
    let mut head = vec![0u8; take];
    if take > 0 {
        file.read_exact(&mut head)
            .map_err(|e| format!("读取文件失败: {}", e))?;
    }
    Ok((file, head))
}

fn feed_decoder_strict(
    decoder: &mut encoding_rs::Decoder,
    mut src: &[u8],
    pending: &mut String,
    last: bool,
    label: &str,
) -> Result<(), String> {
    while !src.is_empty() {
        pending.reserve(READ_CHUNK.clamp(256, 4096));
        let (result, read) = decoder.decode_to_string_without_replacement(src, pending, false);
        match result {
            DecoderResult::Malformed(_, _) => {
                return Err(format!(
                    "解码失败：检测到非法字节序列或与声明编码不一致（{}）。可尝试 encoding=auto，或改用 gb18030 / big5 / utf-8-sig 等。",
                    label
                ));
            }
            DecoderResult::OutputFull => continue,
            DecoderResult::InputEmpty => src = &src[read..],
        }
    }
    if last {
        loop {
            pending.reserve(64);
            let (result, _read) = decoder.decode_to_string_without_replacement(b"", pending, true);
            match result {
                DecoderResult::Malformed(_, _) => {
                    return Err(format!(
                        "解码失败：流末尾存在不完整或非法序列（{}）。",
                        label
                    ));
                }
                DecoderResult::OutputFull => continue,
                DecoderResult::InputEmpty => break,
            }
        }
    }
    Ok(())
}

/// 将整段字节严格解码为 `String`（用于不超过内存上限的整文件读取）。
pub fn decode_bytes_strict(
    bytes: &[u8],
    hint: TextEncodingName,
) -> Result<(String, DecodedFileNote), String> {
    let (resolved, note) = resolve_text_encoding(bytes, hint)?;
    match resolved {
        ResolvedTextEncoding::Utf8Strict => {
            let s = std::str::from_utf8(bytes).map_err(|e| {
                format!(
                    "按 UTF-8 解码失败：{}（字节偏移 {}）。请指定 encoding（如 gb18030、big5）或使用 auto。",
                    e,
                    e.valid_up_to()
                )
            })?;
            Ok((s.to_string(), note))
        }
        ResolvedTextEncoding::Utf8Sig { skip_bom } => {
            let slice = bytes.get(skip_bom..).unwrap_or(bytes);
            let s = std::str::from_utf8(slice).map_err(|e| {
                format!(
                    "按 UTF-8（已跳过 {} 字节 BOM）解码失败：{}（相对切片偏移 {}）。请改用其它 encoding 或 auto。",
                    skip_bom,
                    e,
                    e.valid_up_to()
                )
            })?;
            Ok((s.to_string(), note))
        }
        ResolvedTextEncoding::Decoder { encoding, label } => {
            let skip = bom_encoding_and_skip(bytes)
                .filter(|(e, _)| *e == encoding)
                .map(|(_, s)| s)
                .unwrap_or(0);
            let payload = bytes.get(skip..).unwrap_or(&[]);
            let mut decoder = encoding.new_decoder();
            let mut out = String::new();
            feed_decoder_strict(&mut decoder, payload, &mut out, true, label)?;
            Ok((out, note))
        }
    }
}

/// 流式解码并回调每一逻辑行（以 `\n` 分段；最后一行若无 `\n` 也会回调）。返回 `(最后行号, 解码说明)`。
pub fn for_each_decoded_line<F>(
    path: &Path,
    hint: TextEncodingName,
    mut on_line: F,
) -> Result<(usize, DecodedFileNote), String>
where
    F: FnMut(usize, &str) -> std::ops::ControlFlow<()>,
{
    let (mut file, head) = open_file_and_read_head(path, SNIFF_MAX_BYTES)?;
    let (resolved, note) = resolve_text_encoding(&head, hint)?;

    let ResolvedTextEncoding::Decoder { encoding, label } = resolved else {
        return Err("内部错误：for_each_decoded_line 仅用于非 UTF-8 解码路径".to_string());
    };

    let skip_bom = bom_encoding_and_skip(&head)
        .filter(|(e, _)| *e == encoding)
        .map(|(_, s)| s)
        .unwrap_or(0);

    let mut decoder = encoding.new_decoder();
    let mut pending = String::new();
    let mut line_no = 0usize;

    let first = if head.len() > skip_bom {
        &head[skip_bom..]
    } else {
        &[][..]
    };
    feed_decoder_strict(&mut decoder, first, &mut pending, false, label)?;

    let mut drain_lines =
        |pending: &mut String, on_line: &mut F| -> Result<std::ops::ControlFlow<()>, String> {
            while let Some(pos) = pending.find('\n') {
                line_no += 1;
                let mut line = pending[..pos].to_string();
                if line.ends_with('\r') {
                    line.pop();
                }
                pending.drain(..=pos);
                if on_line(line_no, &line).is_break() {
                    return Ok(std::ops::ControlFlow::Break(()));
                }
            }
            Ok(std::ops::ControlFlow::Continue(()))
        };

    if drain_lines(&mut pending, &mut on_line)?.is_break() {
        return Ok((line_no, note));
    }

    let mut chunk = vec![0u8; READ_CHUNK];
    loop {
        let n = file
            .read(&mut chunk)
            .map_err(|e| format!("读取文件失败: {}", e))?;
        if n == 0 {
            feed_decoder_strict(&mut decoder, b"", &mut pending, true, label)?;
            break;
        }
        feed_decoder_strict(&mut decoder, &chunk[..n], &mut pending, false, label)?;
        if drain_lines(&mut pending, &mut on_line)?.is_break() {
            return Ok((line_no, note));
        }
    }

    if !pending.is_empty() {
        line_no += 1;
        let mut line = pending;
        if line.ends_with('\r') {
            line.pop();
        }
        let _ = on_line(line_no, &line);
    }

    Ok((line_no, note))
}

/// 解码后的总行数（与 `read_file` 行语义一致）。
pub fn count_decoded_lines(path: &Path, hint: TextEncodingName) -> Result<usize, String> {
    let (last_no, _) =
        for_each_decoded_line(path, hint, |_, _| std::ops::ControlFlow::Continue(()))?;
    Ok(last_no)
}
