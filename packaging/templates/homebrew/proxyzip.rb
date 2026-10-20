cask "proxyzip" do
  arch arm: "aarch64", intel: "x64"

  version "{{version}}"
  sha256 arm:   "{{sha256:ProxyZip_{{version}}_aarch64.dmg}}",
         intel: "{{sha256:ProxyZip_{{version}}_x64.dmg}}"

  url "https://github.com/LuisPeixoto/ProxyZip/releases/download/v#{version}/ProxyZip_#{version}_#{arch}.dmg"
  name "ProxyZip"
  desc "Capture, inspect and debug HTTP(S) traffic"
  homepage "https://github.com/LuisPeixoto/ProxyZip"

  livecheck do
    url :url
    strategy :github_latest
  end

  app "ProxyZip.app"

  zap trash: [
    "~/Library/Application Support/ProxyZip",
    "~/Library/Application Support/dev.proxyzip.app",
    "~/Library/Caches/dev.proxyzip.app",
    "~/Library/WebKit/dev.proxyzip.app",
  ]
end
