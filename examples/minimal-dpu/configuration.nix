{ ... }:

{
  system.stateVersion = "25.11";
  networking.hostName = "bluefield-dpu";

  bluefield = {
    enable = true;

    boot = {
      removableEfi = true;
      promoteEfiBootEntry = false;
      copyKernels = true;
    };

    credentials.requireKeys = false;
  };
}
