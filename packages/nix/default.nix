{ lib
, rustPlatform
, fetchFromGitHub
}:

rustPlatform.buildRustPackage rec {
  pname = "mp3rgain";
  version = "1.1.1";

  src = fetchFromGitHub {
    owner = "M-Igashi";
    repo = "mp3rgain";
    rev = "v${version}";
    hash = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
  };

  cargoHash = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

  meta = with lib; {
    description = "Lossless MP3 volume adjustment - a modern mp3gain replacement written in Rust";
    homepage = "https://github.com/M-Igashi/mp3rgain";
    license = licenses.mit;
    maintainers = with maintainers; [ ];
    mainProgram = "mp3rgain";
    platforms = platforms.all;
  };
}
