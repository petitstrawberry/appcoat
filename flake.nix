{
  description = "appcoat: upstream Rust host tools and optional artifact packaging";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/3e41b24abd260e8f71dbe2f5737d24122f972158";
  outputs = { self, nixpkgs }: let
    systems = [ "aarch64-darwin" "x86_64-darwin" "aarch64-linux" "x86_64-linux" ];
    eachSystem = nixpkgs.lib.genAttrs systems;
    mk = system: let
      pkgs = import nixpkgs { inherit system; };
      src = pkgs.lib.cleanSourceWith {
        src = ./.;
        filter = path: type: !(builtins.elem (baseNameOf path) [ "target" "dist" ".cargo-local" ".git" "build" "result" ]);
      };
      common = {
        version = "0.1.0";
        inherit src;
        cargoLock.lockFile = ./Cargo.lock;
      };
      appcoat = pkgs.rustPlatform.buildRustPackage (common // {
        pname = "appcoat";
        cargoBuildFlags = [ "-p" "appcoat_cli" ];
        cargoTestFlags = [ "--workspace" ];
      });
      reader = pkgs.rustPlatform.buildRustPackage (common // {
        pname = "appcoat-resource-reader";
        cargoBuildFlags = [ "-p" "appcoat_resource_reader" ];
        cargoTestFlags = [ "-p" "appcoat_runtime" ];
      });
      customC = pkgs.stdenv.mkDerivation {
        pname = "appcoat-custom-c";
        version = "0.1.0";
        src = ./examples/custom-c;
        buildPhase = ''$CC -std=c11 -Wall -Wextra -Werror main.c -o custom-c'';
        installPhase = ''mkdir -p "$out/bin"; cp custom-c "$out/bin/custom-c"'';
      };
      platform = if pkgs.stdenv.isDarwin then "macos" else "linux";
      package = artifact: manifest: slug: self.lib.mkArtifactPackage {
        inherit pkgs appcoat src platform manifest slug;
        inherit artifact;
      };
      readerPackage = package "${reader}/bin/appcoat_resource_reader" "examples/resource-reader/appcoat.toml" "resource-reader";
      cPackage = package "${customC}/bin/custom-c" "examples/custom-c/appcoat.toml" "custom-c";
      packageName = slug: if platform == "linux" then slug else "${slug}.app";
      launchCheck = pkgs.runCommand "appcoat-host-launch-check" {
        nativeBuildInputs = [ appcoat pkgs.gnugrep ];
      } ''
        appcoat run --package ${readerPackage}/${packageName "resource-reader"} -- "Nix argument" >rust-output
        grep -F "Hello from an appcoat resource!" rust-output
        grep -F "arg=Nix argument" rust-output
        appcoat run --package ${cPackage}/${packageName "custom-c"} >c-output
        grep -F "Hello from C, built without Cargo!" c-output
        touch "$out"
      '';
    in {
      packages = { default = appcoat; inherit appcoat; resource-reader = readerPackage; custom-c = cPackage; };
      checks = { unit = appcoat; resource-reader = readerPackage; custom-c = cPackage; host-launch = launchCheck; };
      devShell = pkgs.mkShell { packages = with pkgs; [ cargo rustc rustfmt clippy stdenv.cc python3 ]; };
    };
  in {
    # No Scarlet input, target-specific Cargo dependency or alternate compiler in this flake.
    lib.mkArtifactPackage = import ./nix/package.nix;
    packages = eachSystem (system: (mk system).packages);
    checks = eachSystem (system: (mk system).checks);
    devShells = eachSystem (system: { default = (mk system).devShell; });
    apps = eachSystem (system: { default = { type = "app"; program = "${self.packages.${system}.appcoat}/bin/appcoat"; }; });
  };
}
