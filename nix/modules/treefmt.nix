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
          "crates/search/src/buffer_search/**"
          "crates/search/src/project_search/**"
          "crates/markdown_preview/src/markdown_preview_view/**"
          "crates/agent_servers/src/acp/**"
          "crates/gpui_linux/src/linux/wayland/client/**"
          "crates/gpui_linux/src/linux/x11/client/**"
          "crates/gpui_linux/src/linux/x11/window/**"
        ];
        settings.formatter.rustfmt.excludes = [
          "crates/collab/tests/integration/editor_diagnostics_refresh_tail.rs"
          "crates/agent/src/agent_tests/**"
          "crates/agent/src/tools/terminal_tool_tests/**"
          "crates/search/src/buffer_search/**"
          "crates/search/src/project_search/**"
          "crates/markdown_preview/src/markdown_preview_view/**"
          "crates/agent_servers/src/acp/**"
          "crates/gpui_linux/src/linux/wayland/client/**"
          "crates/gpui_linux/src/linux/x11/client/**"
          "crates/gpui_linux/src/linux/x11/window/**"
        ];
      };
    };
}
