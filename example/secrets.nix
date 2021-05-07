let
  age = "age1wl3fqfvyml0c5eaj00j0frad4vhspgx9t8sngq4342j7rzjw4pqs80euxk";
  sshEd25519 = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILoPdkEfhcsmW6Lg86GMrEJZnYfFBb7fL9G/IXK7pDQd";
  sshRsa = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQDHd3yBYhZbBkMqycy/SOgx9d79TV5Q76czfkmKUKVzywUJbJCwZ4wMA+ff7QzBufZRoAWpGeQb+rssLQEOwR+VX30Fw7K92W4kK6BCF5phP6AUCo07e3vjGqKvgJ4+8LYvcCB17bYf8pJhb4GoOGLrlJNKbGZOhfYE0eGFu/fWsVybQasC2naieKfqHOwS9kNK0N1gSnWh0qu3Du9vBAbQBEE13mPGe4zEdIzTogM068xgKhfJUWqu1xCyVBVJNdz9Xw0NLaWQJon8YXDe62ifxLj3LgndwKm91cN9mmL0klcGB5O8K2mPE0ZGFMDuxdcllUchQgYXdNxEWB4EvpkvpQbiO+fjgMpHeEEiNPd/v06amSBqK+QlIGEkPAElELphPLiTJmHVqxc5NaffVc7F+zM+c3+aWB5Fqgk1jcnqm8HmlLEvPPT1S00c80SkY1V3lUUOirFlciP/pEivJejA5Yj2i1NEEELnrCdBw/xQ4jfesIxcqmBhxk5dWeBbfGs=";
in
{
  "root.passwd.age" = {
    publicKeys = [ age sshEd25519 sshRsa ];
    extra = "Additional attributes are perfectly fine";
  };
  "github-runner.token.age".publicKeys = [ age sshEd25519 sshRsa ];
}
