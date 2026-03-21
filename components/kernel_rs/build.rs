fn main() {
    // Only run ESP-IDF build integration when targeting ESP
    #[cfg(target_os = "espidf")]
    embuild::espidf::sysenv::output();
}
