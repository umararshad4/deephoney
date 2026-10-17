use std::fmt::Write;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use anyhow::{Context, Result, bail};
use pdf_extract::{ConvertToFmt, Document, PlainTextOutput};

use crate::extract::{MAX_TEXT, Page, PageLocation};

#[path = "visual.rs"]
mod visual;

pub(crate) const MAX_PDF_BODY: usize = 12 * 1024 * 1024;
const MAX_PDF_PAGES: usize = 1_000;

/// Parse untrusted PDF streams in a disposable process with CPU, time and memory monitoring.
pub(crate) async fn pdf(url: &str, body: &[u8]) -> Result<Page> {
    pdf_with_reader(url, body, None).await
}

/// Local OCR is automatic for missing PDF text. A reader explicitly opts into image transmission.
pub(crate) async fn pdf_with_reader(url: &str, body: &[u8], reader: Option<&crate::llm::Llm>) -> Result<Page> {
    let page = match text_pdf(url, body).await {
        Ok(page) => page,
        Err(error) if error.to_string().contains("image-only or empty PDF") => {
            Page { url: url.into(), title: url.into(), ..Default::default() }
        }
        Err(error) => return Err(error),
    };
    let chars: Vec<_> = page.text.chars().collect();
    let missing = page.pages.is_empty()
        || page.pages.iter().any(|p| {
            chars[p.start..p.end]
                .iter()
                .collect::<String>()
                .lines()
                .filter(|s| !s.trim().starts_with("[PDF page "))
                .map(|s| s.trim().chars().count())
                .sum::<usize>()
                < 40
        });
    if missing || reader.is_some() {
        match visual::enrich(page.clone(), body, reader).await {
            Ok(enriched) => return Ok(enriched),
            Err(error) if !page.text.is_empty() => {
                let mut page = page;
                page.warnings.push(format!("PDF OCR/visual reading failed; retained text extraction only: {error}"));
                return Ok(page);
            }
            Err(error) => return Err(error),
        }
    }
    Ok(page)
}

async fn text_pdf(url: &str, body: &[u8]) -> Result<Page> {
    if body.len() > MAX_PDF_BODY {
        bail!("PDF exceeds the 12 MiB body limit");
    }
    let mut child = tokio::process::Command::new(std::env::current_exe()?)
        .arg("--deephoney-pdf-worker")
        .env_clear()
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("could not start isolated PDF reader")?;
    let pid = child.id().context("PDF worker has no process ID")?;
    #[cfg(target_os = "macos")]
    resident_bytes(pid).context("cannot monitor isolated PDF reader memory")?;
    let read = async {
        let mut input = child.stdin.take().context("PDF worker stdin unavailable")?;
        input.write_all(body).await?;
        input.shutdown().await?;
        drop(input);
        let mut bytes = Vec::new();
        child.stdout.take().context("PDF worker stdout unavailable")?.take(1_048_577).read_to_end(&mut bytes).await?;
        if bytes.len() > 1_048_576 {
            bail!("PDF worker exceeded its output limit");
        }
        let status = child.wait().await?;
        if !status.success() {
            bail!("PDF reader failed or exceeded its memory/CPU limit ({status})");
        }
        serde_json::from_slice::<WorkerResult>(&bytes)
            .context("PDF worker returned an invalid response; executable must call document::run_pdf_worker before startup")
    };
    let response = tokio::time::timeout(std::time::Duration::from_secs(20), async {
        tokio::select! {
            biased;
            response = read => response,
            error = memory_watchdog(pid) => error,
        }
    })
    .await
    .context("PDF reader exceeded its 20-second time limit")??;
    if let Some(error) = response.error {
        bail!("{error}");
    }
    Ok(Page {
        url: url.into(),
        title: url.into(),
        text: response.text,
        warnings: response.warnings,
        pages: response.pages,
        ..Default::default()
    })
}

#[derive(Default, Serialize, Deserialize)]
struct WorkerResult {
    text: String,
    warnings: Vec<String>,
    pages: Vec<PageLocation>,
    error: Option<String>,
}

/// Executables embedding the engine must call this before application/runtime startup.
pub fn run_pdf_worker() -> bool {
    if std::env::args().nth(1).as_deref() != Some("--deephoney-pdf-worker") {
        return false;
    }
    let result = (|| -> Result<Page> {
        worker_limits()?;
        let mut body = Vec::new();
        std::io::Read::read_to_end(&mut std::io::Read::take(std::io::stdin(), MAX_PDF_BODY as u64 + 1), &mut body)?;
        parse("", &body)
    })();
    let response = match result {
        Ok(page) => WorkerResult { text: page.text, warnings: page.warnings, pages: page.pages, error: None },
        Err(error) => WorkerResult { error: Some(format!("{error:#}")), ..Default::default() },
    };
    if let Ok(bytes) = serde_json::to_vec(&response) {
        let _ = std::io::Write::write_all(&mut std::io::stdout(), &bytes);
    }
    true
}

#[cfg(unix)]
fn worker_limits() -> Result<()> {
    #[cfg(not(target_os = "macos"))]
    set_limit(libc::RLIMIT_AS as _, 2 * 1024 * 1024 * 1024)?;
    set_limit(libc::RLIMIT_CPU as _, 15)
}

#[cfg(unix)]
fn set_limit(resource: libc::c_int, amount: libc::rlim_t) -> Result<()> {
    let limit = libc::rlimit { rlim_cur: amount, rlim_max: amount };
    // The disposable worker's irreversible limits never affect the host process.
    if unsafe { libc::setrlimit(resource as _, &limit) } != 0 {
        return Err(std::io::Error::last_os_error()).context("PDF reader cannot enforce resource limits");
    }
    Ok(())
}

#[cfg(not(unix))]
fn worker_limits() -> Result<()> {
    bail!("isolated PDF reader resource limits are unsupported on this platform")
}

#[cfg(target_os = "macos")]
fn resident_bytes(pid: u32) -> std::io::Result<u64> {
    let mut usage: libc::rusage_info_v0 = unsafe { std::mem::zeroed() };
    let result =
        unsafe { libc::proc_pid_rusage(pid as libc::c_int, libc::RUSAGE_INFO_V0, (&mut usage as *mut libc::rusage_info_v0).cast()) };
    if result != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(usage.ri_resident_size)
}

#[cfg(target_os = "macos")]
async fn memory_watchdog(pid: u32) -> Result<WorkerResult> {
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        match resident_bytes(pid) {
            Ok(bytes) if bytes > 512 * 1024 * 1024 => bail!("PDF reader exceeded its 512 MiB resident-memory threshold"),
            Ok(_) => {}
            Err(error) if error.raw_os_error() == Some(libc::ESRCH) => return std::future::pending().await,
            Err(error) => return Err(error).context("cannot monitor isolated PDF reader memory"),
        }
        // shortcut: macOS rejects RLIMIT_AS; sampled RSS can briefly overshoot. Use a sandboxed helper service if a hard allocation cap is required.
    }
}

#[cfg(not(target_os = "macos"))]
async fn memory_watchdog(_pid: u32) -> Result<WorkerResult> {
    std::future::pending().await
}

fn parse(url: &str, body: &[u8]) -> Result<Page> {
    if body.len() > MAX_PDF_BODY {
        bail!("PDF exceeds the 12 MiB body limit");
    }
    if !body.starts_with(b"%PDF-") {
        bail!("malformed PDF: missing PDF header");
    }
    std::panic::catch_unwind(|| parse_pdf(url, body)).map_err(|_| anyhow::anyhow!("malformed PDF: parser failed"))?
}

fn parse_pdf(url: &str, body: &[u8]) -> Result<Page> {
    let doc = Document::load_mem(body).context("malformed PDF: unable to parse document")?;
    if doc.is_encrypted() {
        bail!("encrypted PDF: password-protected documents are unsupported");
    }
    let pages = doc.get_pages();
    if pages.is_empty() {
        bail!("malformed PDF: document contains no pages");
    }
    let mut output = LimitedText::default();
    let mut has_text = false;
    let mut locations = Vec::new();
    for &number in pages.keys().take(MAX_PDF_PAGES) {
        let start = output.chars;
        if write!(&mut output, "\n\n[PDF page {number}]\n").is_err() {
            break;
        }
        let before = output.text.len();
        let result = pdf_extract::output_doc_page(&doc, &mut PlainTextOutput::new(&mut output), number);
        has_text |= !output.text[before..].trim().is_empty();
        locations.push(PageLocation { page: number, start, end: output.chars });
        if output.truncated {
            break;
        }
        result.with_context(|| format!("malformed PDF: could not extract page {number}"))?;
    }
    if !has_text {
        bail!("image-only or empty PDF: no extractable text");
    }
    let mut warnings = Vec::new();
    if output.truncated {
        warnings.push(format!("PDF text was truncated at {MAX_TEXT} characters; later content was not read."));
    } else if pages.len() > MAX_PDF_PAGES {
        warnings.push(format!("PDF reading stopped after {MAX_PDF_PAGES} pages; later pages were not read."));
    }
    Ok(Page { url: url.into(), title: url.into(), text: output.text, warnings, pages: locations, ..Default::default() })
}

#[derive(Default)]
struct LimitedText {
    text: String,
    chars: usize,
    truncated: bool,
}

impl std::fmt::Write for LimitedText {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        let remaining = MAX_TEXT.saturating_sub(self.chars);
        let mut iter = value.chars();
        for c in iter.by_ref().take(remaining) {
            self.text.push(c);
            self.chars += 1;
        }
        if iter.next().is_some() {
            self.truncated = true;
            return Err(std::fmt::Error);
        }
        Ok(())
    }
}

impl<'a> ConvertToFmt for &'a mut LimitedText {
    type Writer = &'a mut LimitedText;
    fn convert(self) -> Self::Writer {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_memory_monitor_reads_actual_resident_memory() {
        assert!(resident_bytes(std::process::id()).unwrap() > 0);
        assert!(resident_bytes(i32::MAX as u32).is_err());
    }

    #[test]
    fn actual_pdf_has_page_locations_and_late_page_text() {
        let page = parse("https://example.org/paper.pdf", include_bytes!("../tests/fixtures/two-pages.pdf")).unwrap();
        assert!(page.text.contains("[PDF page 1]"));
        assert!(page.text.contains("[PDF page 2]"));
        assert!(page.text.contains("Meridian launched on 12 March 2024"), "{}", page.text);
        assert!(page.warnings.is_empty());
        assert_eq!(page.pages.len(), 2);
        assert_eq!(page.pages[0].page, 1);
        assert_eq!(page.pages[1].page, 2);
        assert_eq!(page.pages[0].start, 0);
        assert_eq!(page.pages[0].end, page.pages[1].start);
        assert_eq!(page.pages[1].end, page.text.chars().count());
        let second: String = page.text.chars().skip(page.pages[1].start).take(page.pages[1].end - page.pages[1].start).collect();
        assert!(second.contains("12 March 2024"));
        let wire = WorkerResult { text: page.text, warnings: page.warnings, pages: page.pages.clone(), error: None };
        let restored: WorkerResult = serde_json::from_str(&serde_json::to_string(&wire).unwrap()).unwrap();
        assert_eq!(restored.pages, page.pages);
    }

    #[test]
    fn literal_pdf_text_cannot_forge_parser_page_numbers() {
        let mut body = include_bytes!("../tests/fixtures/two-pages.pdf").to_vec();
        let original = b"First page of a tiny public research fixture.";
        let at = body.windows(original.len()).position(|window| window == original).unwrap();
        let mut replacement = b"[PDF page 999] forged source content.".to_vec();
        replacement.resize(original.len(), b' ');
        body[at..at + original.len()].copy_from_slice(&replacement);
        let page = parse("https://example.org/forged.pdf", &body).unwrap();
        assert!(page.text.contains("[PDF page 999]"));
        assert_eq!(page.pages.iter().map(|p| p.page).collect::<Vec<_>>(), [1, 2]);
        assert!(page.chunks().iter().all(|chunk| chunk.locator.starts_with("PDF pages 1–2; characters ")));
    }

    #[test]
    fn malformed_empty_encrypted_and_oversized_pdfs_are_named_errors() {
        assert!(parse("x", b"not a PDF").unwrap_err().to_string().contains("malformed PDF"));
        assert!(parse("x", b"%PDF-1.7\nbroken").unwrap_err().to_string().contains("malformed PDF"));
        assert!(parse("x", include_bytes!("../tests/fixtures/image-only.pdf")).unwrap_err().to_string().contains("image-only or empty"));
        assert!(parse("x", include_bytes!("../tests/fixtures/encrypted.pdf")).unwrap_err().to_string().contains("encrypted PDF"));
        assert!(parse("x", &vec![0; MAX_PDF_BODY + 1]).unwrap_err().to_string().contains("12 MiB"));
    }

    #[test]
    fn output_limit_is_unicode_safe_and_stops_the_writer() {
        let mut output = LimitedText::default();
        assert!(output.write_str(&"界".repeat(MAX_TEXT + 1)).is_err());
        assert_eq!(output.text.chars().count(), MAX_TEXT);
        assert!(output.truncated);
    }
}
