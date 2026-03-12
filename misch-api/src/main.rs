use rocket::launch;

#[launch]
/// Rocket launch hook for the API binary.
fn rocket() -> _ {
    misch_api::build_rocket()
}
