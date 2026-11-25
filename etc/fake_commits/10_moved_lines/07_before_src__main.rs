fn main() {
    let a = 1;
    let b = 2;
    let settings = Settings::read_from_the_configuration_file();
    let logger = Logger::open_in_the_state_directory(&settings);
    let terminal = Terminal::enter_the_alternate_screen(&settings);
    run(settings, logger, terminal, a, b);
}
