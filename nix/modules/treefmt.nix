{
  perSystem =
    { pkgs, ... }:
    {
      treefmt = {
        programs.nixfmt.enable = true;
        programs.rustfmt.enable = true;
        settings.global.excludes = [
          "crates/debugger_ui/src/tests/**"
          "crates/agent/src/agent_tests/**"
          "crates/agent/src/tools/terminal_tool_tests/**"
        ];
        settings.formatter.rustfmt.excludes = [
          "crates/collab/tests/integration/editor_diagnostics_refresh_tail.rs"
          "crates/agent/src/agent_tests/**"
          "crates/agent/src/tools/terminal_tool_tests/**"
        ];
      };
    };
}
