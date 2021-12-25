let
  sshEd25519 = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILoPdkEfhcsmW6Lg86GMrEJZnYfFBb7fL9G/IXK7pDQd";
  plugin = "age1unencrypted1k5fr0r";
in
{
  "unencrypted.age".publicKeys = [ plugin sshEd25519 ];
}
