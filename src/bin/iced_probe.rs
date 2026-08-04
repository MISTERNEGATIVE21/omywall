use iced::widget::{button, column, container, mouse_area, rule, row, text, text_input};
use iced::{application, theme::Theme, window, Element, Subscription, Task};

#[derive(Debug, Clone)]
enum Message {
    #[allow(dead_code)]
    Tick(std::time::Instant),
    Clicked,
    Entered,
    Exited,
    Typed(String),
    GotImage(Result<(u32, u32, Vec<u8>), String>),
}

#[derive(Debug, Clone)]
struct App {
    input: String,
    image: Option<(u32, u32, Vec<u8>)>,
}

fn update(app: &mut App, msg: Message) -> Task<Message> {
    match msg {
        Message::Tick(_) => {
            if app.image.is_none() {
                return Task::perform(
                    async { Ok::<(u32, u32, Vec<u8>), String>((2, 2, vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255])) },
                    Message::GotImage,
                );
            }
        }
        Message::Clicked => println!("clicked"),
        Message::Entered => println!("entered"),
        Message::Exited => println!("exited"),
        Message::Typed(s) => app.input = s,
        Message::GotImage(Ok(img)) => app.image = Some(img),
        Message::GotImage(Err(e)) => println!("img err {e}"),
    }
    Task::none()
}

fn view(app: &App) -> Element<'_, Message> {
    let content = column![
        text("OMYWALL ICED PROBE").size(20),
        text_input("type", &app.input).on_input(Message::Typed),
        row![button("btn").on_press(Message::Clicked), rule::horizontal(1)],
        if let Some((w, h, px)) = &app.image {
            let im: iced::widget::Image =
                iced::widget::image(iced::widget::image::Handle::from_rgba(*w, *h, px.clone()))
                    .width(80)
                    .height(45);
            let el: Element<'_, Message> = im.into();
            el
        } else {
            text("no image yet").into()
        },
    ]
    .spacing(8)
    .padding(16);

    mouse_area(container(content).width(iced::Length::Fill).height(iced::Length::Fill)).on_enter(Message::Entered)
        .on_exit(Message::Exited)
        .on_press(Message::Clicked)
        .into()
}

fn subscription(_app: &App) -> Subscription<Message> {
    iced::time::every(std::time::Duration::from_millis(500)).map(Message::Tick)
}

fn app_theme(_app: &App) -> Theme {
    Theme::Dark
}

fn main() -> iced::Result {
    application(
        || App {
            input: String::new(),
            image: None,
        },
        update,
        view,
    )
    .window(window::Settings {
        size: iced::Size::new(1240.0, 820.0),
        ..Default::default()
    })
    .title("OMYWALL probe")
    .theme(app_theme)
    .subscription(subscription)
    .run()
}
