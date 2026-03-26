{
  pkgs ? import <nixpkgs> {},
}:
{
  name = "my-project";
  version = "0.1.0";

  buildInputs = with pkgs; [
    rustc
    cargo
  ];

  meta = {
    description = "A sample project";
    license = pkgs.lib.licenses.mit;
    maintainers = [ ];
  };

  shellHook = ''
    echo "Welcome to the dev shell"
  '';
}
