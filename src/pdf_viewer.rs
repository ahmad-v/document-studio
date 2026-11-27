use crate::{
    load,
    pdf_workspace::{
        is_new_pdf_output, is_pdf_input, merge_request_parameter, move_page, page_window,
        parse_page_range, parse_page_selection, serialize_page_order, BoundedPageCache,
        THUMBNAIL_CACHE_CAPACITY,
    },
    render_viewer_batch, ViewerBatch,
};
use iced::{
    widget::{button, column, container, image, row, scrollable, text, text_input},
    ContentFit, Element, Length, Task,
};
use localwright_core::{Job, Request};
use std::{panic::AssertUnwindSafe, path::PathBuf, sync::Arc};

#[derive(Debug, Clone)]
enum Message {
    Open,
    OpenDialogComplete(Option<PathBuf>),
    OpenRecent(PathBuf),
    ExtractCurrentPage,
    ExtractPathChosen(Option<(PathBuf, PathBuf, usize)>),
    Extracted(std::result::Result<String, String>),
    RotateCurrentPage(u16),
    RotatePathChosen(Option<(PathBuf, PathBuf, usize, u16)>),
    Rotated(std::result::Result<(PathBuf, String), String>),
    MovePage(isize),
    SaveReorderedCopy,
    ReorderPathChosen(Option<(PathBuf, PathBuf, String)>),
    Reordered(std::result::Result<(PathBuf, String), String>),
    SplitRangeInput(String),
    SavePageRange,
    SplitPathChosen(Option<(PathBuf, PathBuf, String)>),
    SplitDone(std::result::Result<(PathBuf, String), String>),
    MergePdf,
    MergeInputChosen(PathBuf, Option<PathBuf>),
    MergeOutputChosen(Option<(PathBuf, PathBuf, String, PathBuf)>),
    MergeDone(std::result::Result<(PathBuf, String), String>),
    CancelProcessing,
    Previous,
    Next,
    SelectPage(usize),
    PageInput(String),
    GoToPage,
    ZoomIn,
    ZoomOut,
    ActualSize,
    Fit,
    Cancel,
    Rendered(usize, std::result::Result<Arc<ViewerBatch>, String>),
}

struct Viewer {
    path: PathBuf,
    page: usize,
    pages: usize,
    page_order: Vec<usize>,
    split_range: String,
    preview: Option<image::Handle>,
    preview_size: Option<(u32, u32)>,
    thumbnails: BoundedPageCache<image::Handle>,
    page_input: String,
    zoom: Option<f32>,
    status: String,
    recent: Vec<PathBuf>,
    job: Job,
    processing_job: Job,
    loading: bool,
    processing: bool,
}

fn remember_recent(recent: &mut Vec<PathBuf>, path: PathBuf) {
    recent.retain(|existing| existing != &path);
    recent.insert(0, path);
    recent.truncate(8);
}

fn render_task(
    path: PathBuf,
    display_position: usize,
    source_page: usize,
    thumbnail_pages: Vec<usize>,
    job: Job,
) -> Task<Message> {
    Task::perform(
        async move {
            std::thread::spawn(move || {
                std::panic::catch_unwind(AssertUnwindSafe(|| {
                    render_viewer_batch(&path, source_page, &thumbnail_pages, &job)
                        .map(Arc::new)
                        .map_err(|error| error.message)
                }))
                .unwrap_or_else(|_| Err("The PDF renderer stopped unexpectedly".into()))
            })
            .join()
            .unwrap_or_else(|_| Err("The PDF preview worker stopped unexpectedly".into()))
        },
        move |result| Message::Rendered(display_position, result),
    )
}

fn extract_task(path: PathBuf, output: PathBuf, page: usize, job: Job) -> Task<Message> {
    Task::perform(
        async move {
            std::thread::spawn(move || {
                std::panic::catch_unwind(AssertUnwindSafe(|| {
                    crate::handle(
                        Request {
                            operation: "pdf-split".into(),
                            input: path,
                            output,
                            parameter: format!("{page}-{page}"),
                            ..Request::default()
                        },
                        job,
                    )
                    .map(|outcome| outcome.preview)
                    .map_err(|error| error.message)
                }))
                .unwrap_or_else(|_| Err("PDF page extraction stopped unexpectedly".into()))
            })
            .join()
            .unwrap_or_else(|_| Err("PDF extraction worker stopped unexpectedly".into()))
        },
        Message::Extracted,
    )
}

fn rotate_task(
    source: PathBuf,
    output: PathBuf,
    page: usize,
    degrees: u16,
    job: Job,
) -> Task<Message> {
    Task::perform(
        async move {
            std::thread::spawn(move || {
                std::panic::catch_unwind(AssertUnwindSafe(|| {
                    crate::handle(
                        Request {
                            operation: "pdf-rotate".into(),
                            input: source,
                            output: output.clone(),
                            parameter: format!("{page}-{page},{degrees}"),
                            ..Request::default()
                        },
                        job,
                    )
                    .map(|outcome| (output, outcome.preview))
                    .map_err(|error| error.message)
                }))
                .unwrap_or_else(|_| Err("PDF rotation stopped unexpectedly".into()))
            })
            .join()
            .unwrap_or_else(|_| Err("PDF rotation worker stopped unexpectedly".into()))
        },
        Message::Rotated,
    )
}

fn reorder_task(source: PathBuf, output: PathBuf, order: String, job: Job) -> Task<Message> {
    Task::perform(
        async move {
            std::thread::spawn(move || {
                std::panic::catch_unwind(AssertUnwindSafe(|| {
                    crate::handle(
                        Request {
                            operation: "pdf-reorder".into(),
                            input: source,
                            output: output.clone(),
                            parameter: order,
                            ..Request::default()
                        },
                        job,
                    )
                    .map(|outcome| (output, outcome.preview))
                    .map_err(|error| error.message)
                }))
                .unwrap_or_else(|_| Err("PDF reordering stopped unexpectedly".into()))
            })
            .join()
            .unwrap_or_else(|_| Err("PDF reorder worker stopped unexpectedly".into()))
        },
        Message::Reordered,
    )
}

fn page_range_task(source: PathBuf, output: PathBuf, range: String, job: Job) -> Task<Message> {
    Task::perform(
        async move {
            std::thread::spawn(move || {
                std::panic::catch_unwind(AssertUnwindSafe(|| {
                    crate::handle(
                        Request {
                            operation: "pdf-split".into(),
                            input: source,
                            output: output.clone(),
                            parameter: range,
                            ..Request::default()
                        },
                        job,
                    )
                    .map(|outcome| (output, outcome.preview))
                    .map_err(|error| error.message)
                }))
                .unwrap_or_else(|_| Err("PDF page-range extraction stopped unexpectedly".into()))
            })
            .join()
            .unwrap_or_else(|_| Err("PDF page-range worker stopped unexpectedly".into()))
        },
        Message::SplitDone,
    )
}

fn merge_task(source: PathBuf, output: PathBuf, second_input: String, job: Job) -> Task<Message> {
    Task::perform(
        async move {
            std::thread::spawn(move || {
                std::panic::catch_unwind(AssertUnwindSafe(|| {
                    crate::handle(
                        Request {
                            operation: "pdf-merge".into(),
                            input: source,
                            output: output.clone(),
                            parameter: second_input,
                            ..Request::default()
                        },
                        job,
                    )
                    .map(|outcome| (output, outcome.preview))
                    .map_err(|error| error.message)
                }))
                .unwrap_or_else(|_| Err("PDF merge stopped unexpectedly".into()))
            })
            .join()
            .unwrap_or_else(|_| Err("PDF merge worker stopped unexpectedly".into()))
        },
        Message::MergeDone,
    )
}

impl Viewer {
    fn open_path(&mut self, path: PathBuf) -> Task<Message> {
        self.path = path.clone();
        remember_recent(&mut self.recent, path);
        self.page = 1;
        self.pages = 1;
        self.page_order.clear();
        self.split_range = "1-1".into();
        self.page_input = "1".into();
        self.preview = None;
        self.preview_size = None;
        self.thumbnails = BoundedPageCache::default();
        self.zoom = None;
        self.request_page(1)
    }

    fn request_page(&mut self, page: usize) -> Task<Message> {
        if self.loading || page == 0 || page > self.pages {
            return Task::none();
        }
        self.page = page;
        self.page_input = page.to_string();
        self.preview = None;
        self.preview_size = None;
        let actual_page = self.page_order.get(page - 1).copied().unwrap_or(page);
        let visible = page_window(page, self.pages)
            .into_iter()
            .map(|position| {
                self.page_order
                    .get(position - 1)
                    .copied()
                    .unwrap_or(position)
            })
            .collect::<Vec<_>>();
        for visible_page in &visible {
            self.thumbnails.touch(*visible_page);
        }
        let missing = visible
            .into_iter()
            .filter(|thumbnail_page| self.thumbnails.peek(*thumbnail_page).is_none())
            .collect::<Vec<_>>();
        self.loading = true;
        self.job = Job::new();
        self.status = format!("Rendering page {page}…");
        render_task(
            self.path.clone(),
            page,
            actual_page,
            missing,
            self.job.clone(),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Open if !self.loading && !self.processing => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Open a PDF document")
                            .add_filter("PDF document", &["pdf"])
                            .pick_file()
                            .await
                            .map(|file| file.path().to_path_buf())
                    },
                    Message::OpenDialogComplete,
                );
            }
            Message::OpenDialogComplete(Some(path)) if !self.loading && !self.processing => {
                return self.open_path(path);
            }
            Message::OpenRecent(path) if !self.loading && !self.processing => {
                return self.open_path(path);
            }
            Message::ExtractCurrentPage if !self.loading && !self.processing => {
                let page = self
                    .page_order
                    .get(self.page - 1)
                    .copied()
                    .unwrap_or(self.page);
                let source = self.path.clone();
                let file_name = self
                    .path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("document")
                    .to_owned();
                return Task::perform(
                    async move {
                        let default_name = format!("{file_name}-page-{page}.pdf");
                        rfd::AsyncFileDialog::new()
                            .set_title("Save selected PDF page")
                            .add_filter("PDF document", &["pdf"])
                            .set_file_name(&default_name)
                            .save_file()
                            .await
                            .map(|file| (source, file.path().to_path_buf(), page))
                    },
                    Message::ExtractPathChosen,
                );
            }
            Message::ExtractPathChosen(Some((source, output, page)))
                if !self.loading && !self.processing =>
            {
                if !output
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
                {
                    self.status = "Choose a .pdf output path.".into();
                    return Task::none();
                }
                if output == source {
                    self.status = "Choose a new output path to preserve the original PDF.".into();
                    return Task::none();
                }
                self.processing = true;
                self.processing_job = Job::new();
                self.status = format!("Extracting page {page} to a new PDF…");
                return extract_task(source, output, page, self.processing_job.clone());
            }
            Message::Extracted(result) => {
                self.processing = false;
                self.processing_job = Job::new();
                self.status = match result {
                    Ok(message) => format!("{message}; original document preserved."),
                    Err(error) => format!("Could not extract the selected page: {error}"),
                };
            }
            Message::CancelProcessing if self.processing => {
                self.processing_job.cancel();
                self.status = "Document-operation cancellation requested.".into();
            }
            Message::RotateCurrentPage(degrees) if !self.loading && !self.processing => {
                let source = self.path.clone();
                let page = self.page;
                let source_page = self.page_order.get(page - 1).copied().unwrap_or(page);
                let stem = self
                    .path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("document")
                    .to_owned();
                return Task::perform(
                    async move {
                        let default_name = format!("{stem}-page-{source_page}-rotated.pdf");
                        rfd::AsyncFileDialog::new()
                            .set_title("Save rotated PDF copy")
                            .add_filter("PDF document", &["pdf"])
                            .set_file_name(&default_name)
                            .save_file()
                            .await
                            .map(|file| (source, file.path().to_path_buf(), source_page, degrees))
                    },
                    Message::RotatePathChosen,
                );
            }
            Message::RotatePathChosen(Some((source, output, page, degrees)))
                if !self.loading && !self.processing =>
            {
                if !output
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
                {
                    self.status = "Choose a .pdf output path.".into();
                    return Task::none();
                }
                if output == source {
                    self.status = "Choose a new output path to preserve the original PDF.".into();
                    return Task::none();
                }
                self.processing = true;
                self.processing_job = Job::new();
                self.status = format!("Rotating page {page} clockwise by {degrees}°…");
                return rotate_task(source, output, page, degrees, self.processing_job.clone());
            }
            Message::Rotated(result) => {
                self.processing = false;
                self.processing_job = Job::new();
                match result {
                    Ok((output, summary)) => {
                        self.status = format!("{summary}; original document preserved.");
                        return self.open_path(output);
                    }
                    Err(error) => self.status = format!("Could not rotate the page: {error}"),
                }
            }
            Message::MovePage(delta) if !self.loading && !self.processing => {
                if let Some(position) = move_page(&mut self.page_order, self.page, delta) {
                    self.page = position;
                    self.page_input = position.to_string();
                    self.status = format!(
                        "Selected page moved to position {position}; save a reordered copy to apply the new order."
                    );
                }
            }
            Message::SaveReorderedCopy if !self.loading && !self.processing => {
                let Some(order) = serialize_page_order(&self.page_order) else {
                    self.status = "The page order is invalid or exceeds the document limit.".into();
                    return Task::none();
                };
                let source = self.path.clone();
                let stem = self
                    .path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("document")
                    .to_owned();
                return Task::perform(
                    async move {
                        let default_name = format!("{stem}-reordered.pdf");
                        rfd::AsyncFileDialog::new()
                            .set_title("Save reordered PDF copy")
                            .add_filter("PDF document", &["pdf"])
                            .set_file_name(&default_name)
                            .save_file()
                            .await
                            .map(|file| (source, file.path().to_path_buf(), order))
                    },
                    Message::ReorderPathChosen,
                );
            }
            Message::ReorderPathChosen(Some((source, output, order)))
                if !self.loading && !self.processing =>
            {
                if !output
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
                {
                    self.status = "Choose a .pdf output path.".into();
                    return Task::none();
                }
                if output == source {
                    self.status = "Choose a new output path to preserve the original PDF.".into();
                    return Task::none();
                }
                self.processing = true;
                self.processing_job = Job::new();
                self.status = "Saving reordered PDF copy; original document is preserved…".into();
                return reorder_task(source, output, order, self.processing_job.clone());
            }
            Message::Reordered(result) => {
                self.processing = false;
                self.processing_job = Job::new();
                match result {
                    Ok((output, summary)) => {
                        self.status = format!("{summary}; original document preserved.");
                        return self.open_path(output);
                    }
                    Err(error) => self.status = format!("Could not reorder the pages: {error}"),
                }
            }
            Message::SplitRangeInput(value) => self.split_range = value,
            Message::SavePageRange if !self.loading && !self.processing => {
                let Some((start, end)) = parse_page_range(&self.split_range, self.pages) else {
                    self.status = format!(
                        "Enter an inclusive page range from 1 to {} (start-end).",
                        self.pages
                    );
                    return Task::none();
                };
                let source = self.path.clone();
                let stem = self
                    .path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("document")
                    .to_owned();
                let range = format!("{start}-{end}");
                return Task::perform(
                    async move {
                        let default_name = format!("{stem}-pages-{start}-{end}.pdf");
                        rfd::AsyncFileDialog::new()
                            .set_title("Save selected page range as a new PDF")
                            .add_filter("PDF document", &["pdf"])
                            .set_file_name(&default_name)
                            .save_file()
                            .await
                            .map(|file| (source, file.path().to_path_buf(), range))
                    },
                    Message::SplitPathChosen,
                );
            }
            Message::SplitPathChosen(Some((source, output, range)))
                if !self.loading && !self.processing =>
            {
                if source != self.path {
                    self.status = "The open PDF changed while the save dialog was active. Retry the range operation.".into();
                    return Task::none();
                }
                if !is_new_pdf_output(&source, None, &output) {
                    self.status =
                        "Choose a new output path with a lowercase .pdf extension.".into();
                    return Task::none();
                }
                self.processing = true;
                self.processing_job = Job::new();
                self.status = format!("Saving pages {range} to a new PDF; source preserved…");
                return page_range_task(source, output, range, self.processing_job.clone());
            }
            Message::SplitDone(result) => {
                self.processing = false;
                self.processing_job = Job::new();
                self.status = match result {
                    Ok((output, summary)) => {
                        format!("{summary} at {}; source preserved. Structural output may omit annotations, bookmarks, forms, or metadata.", output.display())
                    }
                    Err(error) => format!("Could not save the selected page range: {error}"),
                };
            }
            Message::MergePdf if !self.loading && !self.processing => {
                let source = self.path.clone();
                return Task::perform(
                    async move {
                        rfd::AsyncFileDialog::new()
                            .set_title("Choose a second PDF to merge")
                            .add_filter("PDF document", &["pdf"])
                            .pick_file()
                            .await
                            .map(|file| file.path().to_path_buf())
                    },
                    move |second| Message::MergeInputChosen(source.clone(), second),
                );
            }
            Message::MergeInputChosen(source, Some(second))
                if !self.loading && !self.processing =>
            {
                if source != self.path {
                    self.status =
                        "The open PDF changed while the file dialog was active. Retry the merge."
                            .into();
                    return Task::none();
                }
                if !is_pdf_input(&second) {
                    self.status = "Choose a PDF as the second merge input.".into();
                    return Task::none();
                }
                if source == second {
                    self.status = "Choose a different second PDF.".into();
                    return Task::none();
                }
                let Some(parameter) = merge_request_parameter(&second) else {
                    self.status = "The second PDF path is not valid UTF-8 and cannot be passed safely to the current request interface.".into();
                    return Task::none();
                };
                let stem = source
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("document")
                    .to_owned();
                return Task::perform(
                    async move {
                        let default_name = format!("{stem}-merged.pdf");
                        rfd::AsyncFileDialog::new()
                            .set_title("Save merged PDF copy")
                            .add_filter("PDF document", &["pdf"])
                            .set_file_name(&default_name)
                            .save_file()
                            .await
                            .map(|file| (source, second, parameter, file.path().to_path_buf()))
                    },
                    Message::MergeOutputChosen,
                );
            }
            Message::MergeOutputChosen(Some((source, second, parameter, output)))
                if !self.loading && !self.processing =>
            {
                if source != self.path {
                    self.status =
                        "The open PDF changed while the save dialog was active. Retry the merge."
                            .into();
                    return Task::none();
                }
                if !is_new_pdf_output(&source, Some(&second), &output) {
                    self.status = "Choose a new output path with a lowercase .pdf extension that does not match either input.".into();
                    return Task::none();
                }
                self.processing = true;
                self.processing_job = Job::new();
                self.status = "Experimental page-only merge running; annotations, forms, bookmarks, or document metadata may be lost…".into();
                return merge_task(source, output, parameter, self.processing_job.clone());
            }
            Message::MergeDone(result) => {
                self.processing = false;
                self.processing_job = Job::new();
                self.status = match result {
                    Ok((output, summary)) => format!(
                        "{summary} at {}; source PDFs preserved. Page-only experimental output may omit annotations, forms, bookmarks, and document metadata.",
                        output.display()
                    ),
                    Err(error) => format!("Could not merge the PDFs: {error}"),
                };
            }
            Message::Previous if !self.loading && self.page > 1 => {
                return self.request_page(self.page - 1);
            }
            Message::Next if !self.loading && self.page < self.pages => {
                return self.request_page(self.page + 1);
            }
            Message::SelectPage(page) if !self.loading && page != self.page => {
                return self.request_page(page);
            }
            Message::PageInput(value) => self.page_input = value,
            Message::GoToPage if !self.loading => {
                if let Some(page) = parse_page_selection(&self.page_input, self.pages) {
                    return self.request_page(page);
                }
                self.status = format!("Enter a page from 1 to {}", self.pages);
            }
            Message::ZoomIn => {
                self.zoom = Some((self.zoom.unwrap_or(1.0) * 1.2).min(4.0));
            }
            Message::ZoomOut => {
                self.zoom = Some((self.zoom.unwrap_or(1.0) / 1.2).max(0.5));
            }
            Message::ActualSize => self.zoom = Some(1.0),
            Message::Fit => self.zoom = None,
            Message::Cancel if self.loading => {
                self.job.cancel();
                self.status = "Cancellation requested; stopping between render stages".into();
            }
            Message::Rendered(page, result) => {
                if page == self.page {
                    match result {
                        Ok(batch) => {
                            if self.page_order.len() != batch.page_count {
                                self.page_order = (1..=batch.page_count).collect();
                                self.split_range = format!("1-{}", batch.page_count);
                            }
                            self.preview = Some(image::Handle::from_rgba(
                                batch.preview.width,
                                batch.preview.height,
                                batch.preview.pixels.clone(),
                            ));
                            self.preview_size = Some((batch.preview.width, batch.preview.height));
                            self.pages = batch.page_count;
                            for thumbnail in &batch.thumbnails {
                                self.thumbnails.insert(
                                    thumbnail.page,
                                    image::Handle::from_rgba(
                                        thumbnail.width,
                                        thumbnail.height,
                                        thumbnail.pixels.clone(),
                                    ),
                                );
                            }
                            self.status = format!(
                                "Rendered at {} × {} pixels; thumbnail cache holds at most {} pages",
                                batch.preview.width,
                                batch.preview.height,
                                THUMBNAIL_CACHE_CAPACITY
                            );
                        }
                        Err(error) => {
                            self.preview = None;
                            self.preview_size = None;
                            self.status = format!("Could not preview this page: {error}");
                        }
                    }
                }
                self.loading = false;
                self.job = Job::new();
            }
            _ => {}
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let previous = button("Previous");
        let previous = if !self.loading && self.page > 1 {
            previous.on_press(Message::Previous)
        } else {
            previous
        };
        let next = button("Next");
        let next = if !self.loading && self.page < self.pages {
            next.on_press(Message::Next)
        } else {
            next
        };
        let cancel = button("Cancel rendering");
        let cancel = if self.loading {
            cancel.on_press(Message::Cancel)
        } else {
            cancel
        };
        let extract = button("Extract page…");
        let extract = if !self.loading && !self.processing {
            extract.on_press(Message::ExtractCurrentPage)
        } else {
            extract
        };
        let cancel_processing = button("Cancel operation");
        let cancel_processing = if self.processing {
            cancel_processing.on_press(Message::CancelProcessing)
        } else {
            cancel_processing
        };
        let rotate_clockwise = button("Rotate page 90° CW").on_press_maybe(
            (!self.loading && !self.processing).then_some(Message::RotateCurrentPage(90)),
        );
        let rotate_counterclockwise = button("Rotate page 90° CCW").on_press_maybe(
            (!self.loading && !self.processing).then_some(Message::RotateCurrentPage(270)),
        );
        let source_page = self
            .page_order
            .get(self.page.saturating_sub(1))
            .copied()
            .unwrap_or(self.page);
        let move_earlier = button("Move page earlier").on_press_maybe(
            (!self.loading && !self.processing && self.page > 1).then_some(Message::MovePage(-1)),
        );
        let move_later = button("Move page later").on_press_maybe(
            (!self.loading && !self.processing && self.page < self.pages)
                .then_some(Message::MovePage(1)),
        );
        let save_reordered = button("Save reordered copy…").on_press_maybe(
            (!self.loading && !self.processing && serialize_page_order(&self.page_order).is_some())
                .then_some(Message::SaveReorderedCopy),
        );
        let save_page_range = button("Save range as PDF…")
            .on_press_maybe((!self.loading && !self.processing).then_some(Message::SavePageRange));
        let merge_pdf = button("Merge PDF pages only… (experimental)")
            .on_press_maybe((!self.loading && !self.processing).then_some(Message::MergePdf));
        let go = button("Go");
        let go = if !self.loading {
            go.on_press(Message::GoToPage)
        } else {
            go
        };
        let mut thumbnails = row![].spacing(8).align_y(iced::Alignment::Center);
        for position in page_window(self.page, self.pages) {
            let page = self
                .page_order
                .get(position - 1)
                .copied()
                .unwrap_or(position);
            let selected = position == self.page;
            let label = if selected {
                format!("Page {page}, current position {position}")
            } else {
                format!("Page {page}")
            };
            let item: Element<'_, Message> = if let Some(handle) = self.thumbnails.peek(page) {
                let thumb = image(handle.clone())
                    .content_fit(ContentFit::Contain)
                    .width(Length::Fixed(96.0))
                    .height(Length::Fixed(112.0));
                button(column![thumb, text(label)].align_x(iced::Alignment::Center))
                    .style(if selected {
                        button::primary
                    } else {
                        button::secondary
                    })
                    .on_press_maybe((!self.loading).then_some(Message::SelectPage(position)))
                    .into()
            } else {
                button(
                    column![text("Preview"), text(label)]
                        .width(Length::Fixed(96.0))
                        .height(Length::Fixed(112.0))
                        .align_x(iced::Alignment::Center),
                )
                .style(if selected {
                    button::primary
                } else {
                    button::secondary
                })
                .on_press_maybe((!self.loading).then_some(Message::SelectPage(position)))
                .into()
            };
            thumbnails = thumbnails.push(item);
        }
        let thumbnail_strip = scrollable(thumbnails)
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::default(),
            ))
            .height(Length::Fixed(132.0));
        let open = button("Open PDF")
            .on_press_maybe((!self.loading && !self.processing).then_some(Message::Open));
        let navigation = row![
            open,
            previous,
            text(format!("Page {} of {}", self.page, self.pages)),
            next,
            text_input("Page", &self.page_input)
                .on_input(Message::PageInput)
                .on_submit(Message::GoToPage)
                .width(Length::Fixed(72.0)),
            go,
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        let controls = row![
            button("Zoom −").on_press(Message::ZoomOut),
            button("Zoom +").on_press(Message::ZoomIn),
            button("Actual").on_press(Message::ActualSize),
            button("Fit").on_press(Message::Fit),
            cancel,
            extract,
            rotate_clockwise,
            rotate_counterclockwise,
            cancel_processing,
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        let reorder_controls = row![
            text(format!("Source page {source_page}")),
            move_earlier,
            move_later,
            save_reordered,
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        let structure_controls = row![
            text("Page range"),
            text_input("start-end", &self.split_range)
                .on_input(Message::SplitRangeInput)
                .width(Length::Fixed(112.0)),
            save_page_range,
            merge_pdf,
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        let content: Element<'_, Message> = match &self.preview {
            Some(handle) => {
                let preview = image(handle.clone()).content_fit(ContentFit::Contain);
                let (width, height) = self.preview_size.unwrap_or((800, 1050));
                match self.zoom {
                    Some(scale) => preview
                        .width(Length::Fixed(width as f32 * scale))
                        .height(Length::Fixed(height as f32 * scale))
                        .into(),
                    None => preview.width(Length::Fill).height(Length::Fill).into(),
                }
            }
            None => text(&self.status).into(),
        };
        let recent: Vec<Element<'_, Message>> = self
            .recent
            .iter()
            .map(|path| {
                let label = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("Recent PDF");
                button(text(label).size(12))
                    .on_press_maybe((!self.loading).then_some(Message::OpenRecent(path.clone())))
                    .into()
            })
            .collect();
        column![
            navigation,
            controls,
            reorder_controls,
            structure_controls,
            scrollable(row(recent).spacing(6))
                .direction(scrollable::Direction::Horizontal(
                    scrollable::Scrollbar::default(),
                ))
                .height(Length::Fixed(36.0)),
            text(&self.status),
            thumbnail_strip,
            scrollable(
                container(content)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .padding(8)
            )
            .direction(scrollable::Direction::Both {
                vertical: scrollable::Scrollbar::default(),
                horizontal: scrollable::Scrollbar::default(),
            })
            .height(Length::Fill),
            text("Experimental raster preview. Unsupported PDF features may be missing or differ."),
        ]
        .spacing(10)
        .padding(14)
        .into()
    }
}

pub(super) fn run(path: PathBuf, initial_page: usize) -> std::result::Result<(), String> {
    let pages = std::panic::catch_unwind(AssertUnwindSafe(|| load(&path)))
        .map_err(|_| "PDF parser stopped unexpectedly while opening the preview".to_owned())?
        .map_err(|error| error.message)?
        .get_pages()
        .len();
    if initial_page == 0 || initial_page > pages {
        return Err("Choose an existing 1-based PDF page number".into());
    }
    let mut viewer = Viewer {
        path: path.clone(),
        page: initial_page,
        pages,
        page_order: (1..=pages).collect(),
        split_range: format!("1-{pages}"),
        preview: None,
        preview_size: None,
        thumbnails: BoundedPageCache::default(),
        page_input: initial_page.to_string(),
        zoom: None,
        status: format!("Rendering page {initial_page}…"),
        recent: vec![path.clone()],
        job: Job::new(),
        processing_job: Job::new(),
        loading: true,
        processing: false,
    };
    let visible = page_window(initial_page, pages);
    for visible_page in &visible {
        viewer.thumbnails.touch(*visible_page);
    }
    let missing = visible;
    let initial_task = render_task(
        path,
        initial_page,
        initial_page,
        missing,
        viewer.job.clone(),
    );
    iced::application(
        "Document Studio — PDF Preview",
        Viewer::update,
        Viewer::view,
    )
    .window_size(iced::Size::new(1120.0, 880.0))
    .run_with(move || (viewer, initial_task))
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::remember_recent;
    use std::path::{Path, PathBuf};

    #[test]
    fn recent_documents_are_deduplicated_and_bounded() {
        let mut recent = Vec::new();
        for index in 0..10 {
            remember_recent(&mut recent, PathBuf::from(format!("{index}.pdf")));
        }
        remember_recent(&mut recent, PathBuf::from("5.pdf"));
        assert_eq!(recent.len(), 8);
        assert_eq!(recent[0], PathBuf::from("5.pdf"));
        assert_eq!(
            recent
                .iter()
                .filter(|path| path.as_path() == Path::new("5.pdf"))
                .count(),
            1
        );
        assert!(!recent.contains(&PathBuf::from("0.pdf")));
    }
}
