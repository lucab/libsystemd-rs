use nix;

error_chain::error_chain! {
    foreign_links {
        Io(::std::io::Error);
        Env(::std::env::VarError);
        Ffi(::std::ffi::NulError);
        Nix(nix::Error);
        Parse(::std::num::ParseIntError);
    }
}
