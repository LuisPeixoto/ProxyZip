class ProxyzipCli < Formula
  desc "Command-line debugging proxy: capture, inspect and debug HTTP(S) traffic"
  homepage "https://github.com/LuisPeixoto/ProxyZip"
  version "{{version}}"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/LuisPeixoto/ProxyZip/releases/download/v#{version}/proxyzip-#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "{{sha256:proxyzip-{{version}}-aarch64-apple-darwin.tar.gz}}"
    end
    on_intel do
      url "https://github.com/LuisPeixoto/ProxyZip/releases/download/v#{version}/proxyzip-#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "{{sha256:proxyzip-{{version}}-x86_64-apple-darwin.tar.gz}}"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/LuisPeixoto/ProxyZip/releases/download/v#{version}/proxyzip-#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "{{sha256:proxyzip-{{version}}-aarch64-unknown-linux-gnu.tar.gz}}"
    end
    on_intel do
      url "https://github.com/LuisPeixoto/ProxyZip/releases/download/v#{version}/proxyzip-#{version}-x86_64-unknown-linux-musl.tar.gz"
      sha256 "{{sha256:proxyzip-{{version}}-x86_64-unknown-linux-musl.tar.gz}}"
    end
  end

  def install
    bin.install "proxyzip"
  end

  test do
    assert_match "proxyzip", shell_output("#{bin}/proxyzip --version")
  end
end
