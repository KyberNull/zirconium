{
  pkgs,
  lib,
  inputs,
  config,
  ...
}:
let
  system = pkgs.stdenv.hostPlatform.system;
  linux = pkgs.stdenv.hostPlatform.isLinux;
  darwin = pkgs.stdenv.hostPlatform.isDarwin;

  mdbook = (import inputs.nixpkgs-mdbook { inherit system; }).mdbook;

  rustToolchain = config.languages.rust.toolchainPackage;

  wrappedCargo = pkgs.writeShellApplication {
    name = "cargo";
    runtimeInputs = [ pkgs.nodejs ];
    text = ''
      NIX_WRAPPER=1 CARGO=${rustToolchain}/bin/cargo ${./script/cargo} "$@"
    '';
  };

  muslCross = pkgs.pkgsCross.musl64;

  gpuLib = pkgs.vulkan-loader;

  mkZed = import ./nix/toolchain.nix { inherit inputs; };
  zed-editor = mkZed pkgs;

  nativePackages = [
    pkgs.cmake
    pkgs.curl
    pkgs.perl
    pkgs.pkg-config
    pkgs.protobuf
    pkgs.rustPlatform.bindgenHook
    pkgs.fontconfig
    pkgs.freetype
    pkgs.libgit2
    pkgs.openssl
    pkgs.sqlite
    pkgs.zlib
    pkgs.zstd
    pkgs.git
  ]
  ++ lib.optionals linux [
    pkgs.alsa-lib
    pkgs.glib
    pkgs.libva
    pkgs.libxkbcommon
    pkgs.wayland
    gpuLib
    pkgs.libglvnd
    pkgs.libx11
    pkgs.libxcb
    pkgs.libdrm
    pkgs.libgbm
    pkgs.libxcomposite
    pkgs.libxdamage
    pkgs.libxext
    pkgs.libxfixes
    pkgs.libxrandr
    pkgs.makeWrapper
  ]
  ++ lib.optionals darwin [ pkgs.lld ];
in
{
  cachix.enable = false;

  languages.rust = {
    enable = true;
    toolchainFile = ./rust-toolchain.toml;
    lld.enable = darwin;
  };

  packages = nativePackages ++ [
    (lib.hiPrio wrappedCargo)
    pkgs.cargo-nextest
    pkgs.cargo-hakari
    pkgs.cargo-shear
    pkgs.cargo-zigbuild
    pkgs.nodejs_22
    pkgs.zig
    pkgs.sqlx-cli
    pkgs.minio
    pkgs.livekit
    mdbook
    pkgs.gobject-introspection
    pkgs.at-spi2-core
    (pkgs.python3.withPackages (ps: [
      ps.pyatspi
      ps.pygobject3
    ]))
  ]
  ++ lib.optionals linux [ pkgs.accerciser ];

  env = {
    ZSTD_SYS_USE_PKG_CONFIG = "1";
    FONTCONFIG_FILE = pkgs.makeFontsConf {
      fontDirectories = [
        "./assets/fonts/lilex"
        "./assets/fonts/ibm-plex-sans"
      ];
    };
    PROTOC = "${pkgs.protobuf}/bin/protoc";
    ZED_ZSTD_MUSL_LIB = "${pkgs.pkgsCross.musl64.pkgsStatic.zstd.out}/lib";
    CC_x86_64_unknown_linux_musl = "${muslCross.stdenv.cc}/bin/x86_64-unknown-linux-musl-gcc";
  }
  // lib.optionalAttrs linux {
    NIX_LDFLAGS = "-rpath ${
      lib.makeLibraryPath [
        gpuLib
        pkgs.wayland
        pkgs.libva
      ]
    }";
  }
  // lib.optionalAttrs darwin {
    NIX_CFLAGS_LINK = "-fuse-ld=lld";
  };

  enterShell = ''
    export PATH="${wrappedCargo}/bin:$PATH"
  '';

  scripts.docs.exec = ''
    mdbook build docs
  '';

  processes.blob_store.exec = ''
    mkdir -p .blob_store/the-extensions-bucket .blob_store/zed-crash-reports
    export MINIO_ROOT_USER=the-blob-store-access-key
    export MINIO_ROOT_PASSWORD=the-blob-store-secret-key
    exec ${lib.getExe pkgs.minio} server --quiet .blob_store
  '';

  processes.livekit.exec = ''
    exec ${lib.getExe pkgs.livekit} --config ./livekit.yaml
  '';

  outputs =
    {
      zed-editor = zed-editor;
      debug = zed-editor.override { profile = "dev"; };
    }
    // lib.optionalAttrs linux (
      {
        a11y-test = import ./nix/tests/a11y.nix { inherit pkgs inputs; };
      }
      // import ./nix/tests/sandboxing { inherit pkgs inputs; }
    );
}
