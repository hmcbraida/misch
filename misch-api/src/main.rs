use rocket::launch;

#[launch]
fn rocket() -> _ {
    misch_api::build_rocket()
}
