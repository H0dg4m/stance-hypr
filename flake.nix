{
  description = "stance: bar-first display settings and modes for Hyprland, with confirm-or-revert";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (s: f nixpkgs.legacyPackages.${s});
      version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;
    in
    {
      packages = forAll (pkgs: rec {
        stance = pkgs.rustPlatform.buildRustPackage {
          pname = "stance";
          inherit version;
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          # The unit tests need no compositor; the integration scripts do.
          postInstall = ''
            mkdir -p $out/share/stance $out/lib/systemd/user
            cp -r quickshell/stance $out/share/stance/quickshell
            cp -r quickshell/integrations $out/share/stance/integrations
            cp -r examples $out/share/stance/examples
            cp -r contrib/waybar $out/share/stance/waybar
            for u in contrib/*.service; do
              sed -e "s|/usr/bin/stance|$out/bin/stance|g" \
                  -e "s|/usr/share/stance|$out/share/stance|g" \
                  -e "s|/usr/bin/wl-paste|${pkgs.wl-clipboard}/bin/wl-paste|g" \
                  -e "s|/usr/bin/qs|${pkgs.quickshell}/bin/qs|g" "$u" \
                > $out/lib/systemd/user/$(basename "$u")
            done
          '';
          meta = {
            description = "Bar-first display settings and modes for Hyprland, with confirm-or-revert";
            homepage = "https://github.com/H0dg4m/stance-hypr";
            license = pkgs.lib.licenses.mit;
            platforms = pkgs.lib.platforms.linux;
            mainProgram = "stance";
          };
        };
        default = stance;
      });

      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [ cargo rustc clippy rustfmt quickshell ];
        };
      });
    };
}
