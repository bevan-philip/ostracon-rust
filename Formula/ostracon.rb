# Builds Ostracon from source, so nothing downloaded carries a quarantine flag and Gatekeeper never
# blocks it on a managed Mac.
#
#   brew tap bevan-philip/ostracon-rust https://github.com/bevan-philip/ostracon-rust
#   brew install --HEAD ostracon
#
# The app is installed into the formula's prefix; `brew info ostracon` says how to link it into
# ~/Applications.
class Ostracon < Formula
  desc "Local, Todoist-style to-do list with a CLI and MCP server"
  homepage "https://github.com/bevan-philip/ostracon-rust"
  license "MIT"
  head "https://github.com/bevan-philip/ostracon-rust.git", branch: "main"

  # rustup, not Homebrew's rust: rust-toolchain.toml pins the toolchain GPUI needs.
  depends_on "rustup" => :build
  depends_on :macos

  def install
    ENV.prepend_path "PATH", Formula["rustup"].opt_bin
    system "rustup", "show", "active-toolchain"
    system "scripts/bundle-macos.sh"
    prefix.install "dist/Ostracon.app"
    bin.install_symlink prefix/"Ostracon.app/Contents/MacOS/ostracon-cli" => "ostracon"
    bin.install_symlink prefix/"Ostracon.app/Contents/MacOS/ostracon-mcp"
    (share/"ostracon").install "skills"
  end

  def caveats
    <<~EOS
      To put Ostracon in ~/Applications:
        ln -sf #{opt_prefix}/Ostracon.app ~/Applications/Ostracon.app

      The agent skill is in #{opt_share}/ostracon/skills/ostracon; copy it into ~/.claude/skills/ to use it.
    EOS
  end

  test do
    ENV["OSTRACON_DATABASE"] = testpath/"test.sqlite"
    assert_match "Added", shell_output("#{bin}/ostracon add 'Test task tomorrow'")
    assert_match "Test task", shell_output("#{bin}/ostracon list")
  end
end
