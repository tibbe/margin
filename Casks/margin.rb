cask "margin" do
  # The release workflow sets these.
  version "0.1.0"
  sha256 "0000000000000000000000000000000000000000000000000000000000000000"

  url "https://github.com/tibbe/margin/releases/download/v#{version}/Margin.zip"
  name "Margin"
  desc "Markdown editor with comments that coding agents read and answer"
  homepage "https://github.com/tibbe/margin"

  depends_on macos: :tahoe

  app "Margin.app"
  binary "#{appdir}/Margin.app/Contents/Helpers/margin"

  # Margin is signed ad hoc, not notarized, so Gatekeeper would block the
  # app and kill the CLI. Installing from this tap is trusting its author.
  postflight_steps do
    run "/usr/bin/xattr", args: ["-dr", "com.apple.quarantine", "{{appdir}}/Margin.app"]
  end

  zap trash: [
    "~/Library/Application Support/Margin",
    "~/Library/Preferences/io.github.tibbe.Margin.plist",
    "~/Library/Saved Application State/io.github.tibbe.Margin.savedState",
  ]
end
