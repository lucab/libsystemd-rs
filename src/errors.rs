error_chain::error_chain! {
    foreign_links {
        Io(::std::io::Error);
        Env(::std::env::VarError);
        Parse(::std::num::ParseIntError);
    }
}
