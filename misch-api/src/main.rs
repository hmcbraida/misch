use rocket::launch;

fn install_abort_on_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        default_hook(panic_info);
        std::process::abort();
    }));
}

#[launch]
/// Rocket launch hook for the API binary.
fn rocket() -> _ {
    install_abort_on_panic_hook();
    misch_api::build_rocket()
}
