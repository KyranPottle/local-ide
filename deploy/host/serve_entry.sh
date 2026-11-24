# shellcheck shell=bash
# Status of the Serve entry on SERVE_PORT: 0 forwards to ENGINE_URL only, 1 absent, 2 Funnel, 3 other target
serve_entry_status() {
  tailscale serve status --json |
    SERVE_PORT=$SERVE_PORT ENGINE_URL=$ENGINE_URL perl -MJSON::PP -e '
      local $/;
      my $text = <STDIN>;
      my $config = ($text =~ /\S/) ? decode_json($text) : {};
      my $port = $ENV{SERVE_PORT};
      my $funnel = $config->{AllowFunnel} || {};
      for my $host (keys %$funnel) {
        exit 2 if $host =~ /:\Q$port\E$/ && $funnel->{$host};
      }
      exit 1 unless ($config->{TCP} || {})->{$port};
      my $web = $config->{Web} || {};
      for my $host (keys %$web) {
        next unless $host =~ /:\Q$port\E$/;
        my $handlers = $web->{$host}{Handlers} || {};
        exit 0 if keys(%$handlers) == 1 && (($handlers->{"/"} || {})->{Proxy} // "") eq $ENV{ENGINE_URL};
      }
      exit 3;
    '
}

serve_entry_present() {
  local status=0
  serve_entry_status || status=$?
  case $status in
    0) return 0 ;;
    1) return 1 ;;
    2) fail "port $SERVE_PORT has a Funnel: this script publishes on the tailnet only" ;;
    *) fail "port $SERVE_PORT serves another target: remove that entry by hand first" ;;
  esac
}
