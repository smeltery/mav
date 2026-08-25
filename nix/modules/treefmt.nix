{
  perSystem =
    { pkgs, ... }:
    {
      treefmt = {
        programs.nixfmt.enable = true;
        programs.rustfmt.enable = true;
        settings.formatter.rustfmt.excludes = [
          "crates/collab/tests/integration/editor_diagnostics_refresh_tail.rs"
          "crates/debugger_ui/src/tests/**"
        ];
      };
    };
}
