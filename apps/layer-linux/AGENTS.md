# GTK client

Start with the [Linux guide](../../docs/development/linux.md). GTK is the reference
client: new UI is built and validated here first.

- Native tests need a Wayland session and a hardware GPU. Run them one per process
  through `tools/performance/workspace-motion.sh gtk --native-test=<name>`, which
  starts a private display with isolated settings; never inject input into the
  desktop session.
