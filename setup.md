Configure cloud 
curl -sSL https://sdk.cloud.google.com | bash -s -- --disable-prompts
source ~/google-cloud-sdk/path.bash.inc
gcloud auth login --no-launch-browser
gcloud config set project company-internal-tools-490516


Build binary
cargo build

run engine and socket 

# Terminal 1 — socket server
sudo ./target/debug/agentd socket --path /tmp/agentd.sock

ALL SET
RUN YOU COMMAND
