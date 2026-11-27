use crate::{load, render_page_rgba};
use iced::{
    widget::{button, column, container, image, row, scrollable, text},
    ContentFit, Element, Length, Task,
};
use localwright_core::Job;
use std::{panic::AssertUnwindSafe, path::PathBuf};

#[derive(Debug, Clone)]
enum Message {
    Previous,
    Next,
    Cancel,
    Rendered(usize, std::result::Result<(u32, u32, Vec<u8>), String>),
}

struct Viewer {
    path: PathBuf,
    page: usize,
    pages: usize,
    preview: Option<image::Handle>,
    status: String,
    job: Job,
    loading: bool,
}

fn render_task(path: PathBuf, page: usize, job: Job) -> Task<Message> {
    Task::perform(
        async move {
            std::thread::spawn(move || {
                std::panic::catch_unwind(AssertUnwindSafe(|| {
                    render_page_rgba(&path, page, &job)
                        .map(|(width, height, pixels, _)| (width, height, pixels))
                        .map_err(|error| error.message)
                }))
                .unwrap_or_else(|_| Err("The PDF renderer stopped unexpectedly".into()))
            })
            .join()
            .unwrap_or_else(|_| Err("The PDF preview worker stopped unexpectedly".into()))
        },
        move |result| Message::Rendered(page, result),
    )
}

impl Viewer {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Previous if !self.loading && self.page > 1 => {
                self.page -= 1;
                self.preview = None;
                self.loading = true;
                self.job = Job::new();
                self.status = format!("Rendering page {}…", self.page);
                return render_task(self.path.clone(), self.page, self.job.clone());
            }
            Message::Next if !self.loading && self.page < self.pages => {
                self.page += 1;
                self.preview = None;
                self.loading = true;
                self.job = Job::new();
                self.status = format!("Rendering page {}…", self.page);
                return render_task(self.path.clone(), self.page, self.job.clone());
            }
            Message::Cancel if self.loading => {
                self.job.cancel();
                self.status =
                    "Cancellation requested; stopping after the current renderer stage".into();
            }
            Message::Rendered(page, result) => {
                if page == self.page {
                    match result {
                        Ok((width, height, pixels)) => {
                            self.preview = Some(image::Handle::from_rgba(width, height, pixels));
                            self.status = format!("Rendered at {width} × {height} pixels");
                        }
                        Err(error) => {
                            self.preview = None;
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
        let previous = button("Previous page");
        let previous = if !self.loading && self.page > 1 {
            previous.on_press(Message::Previous)
        } else {
            previous
        };
        let next = button("Next page");
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
        let content: Element<'_, Message> = match &self.preview {
            Some(handle) => image(handle.clone())
                .content_fit(ContentFit::Contain)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
            None => text(&self.status).into(),
        };
        column![
            row![
                previous,
                text(format!("Page {} of {}", self.page, self.pages)),
                next,
                cancel
            ]
            .spacing(12)
            .align_y(iced::Alignment::Center),
            text(&self.status),
            scrollable(
                container(content)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .padding(8)
            )
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
    let viewer = Viewer {
        path: path.clone(),
        page: initial_page,
        pages,
        preview: None,
        status: format!("Rendering page {initial_page}…"),
        job: Job::new(),
        loading: true,
    };
    let initial_task = render_task(path, initial_page, viewer.job.clone());
    iced::application(
        "Document Studio — PDF Preview",
        Viewer::update,
        Viewer::view,
    )
    .window_size(iced::Size::new(980.0, 800.0))
    .run_with(move || (viewer, initial_task))
    .map_err(|error| error.to_string())
}
