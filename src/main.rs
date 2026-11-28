#![forbid(unsafe_code)]
fn main() {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    #[cfg(feature = "gui")]
    if args.first().map(String::as_str) == Some("--read-epub") {
        args.remove(0);
        let Some(input) = args.first() else {
            eprintln!("Usage: document-studio --read-epub <input.epub>");
            std::process::exit(2);
        };
        if let Err(error) = document_studio::run_epub_reader(std::path::Path::new(input)) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    #[cfg(feature = "gui")]
    if args.first().map(String::as_str) == Some("--preview-pdf") {
        args.remove(0);
        let Some(input) = args.first() else {
            eprintln!("Usage: document-studio --preview-pdf <input.pdf> [page]");
            std::process::exit(2);
        };
        let page = args
            .get(1)
            .map(|value| value.parse::<usize>())
            .transpose()
            .unwrap_or_else(|_| {
                eprintln!("Page must be a positive integer");
                std::process::exit(2);
            })
            .unwrap_or(1);
        if let Err(error) = document_studio::run_pdf_preview(std::path::Path::new(input), page) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    if args.first().map(String::as_str) == Some("--cli") {
        args.remove(0);
        if let Err(e) =
            localwright_core::cli(args, document_studio::OPERATIONS, document_studio::handle)
        {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return;
    }
    #[cfg(feature = "gui")]
    if let Err(e) = localwright_ui::run_with_saved_appearance(
        "Document Studio",
        "document-studio",
        document_studio::OPERATIONS,
        document_studio::handle,
    ) {
        eprintln!("{e}");
        std::process::exit(1);
    }
    #[cfg(not(feature = "gui"))]
    {
        eprintln!("Built without GUI; use --cli --help");
        std::process::exit(2);
    }
}
