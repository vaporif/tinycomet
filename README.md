CometBFT ──(unix:/tmp/cmt.sock)──► tinycomet-proxy ──(unix:/tmp/app.sock)──► tinycomet-app
Lab project implementing a small cometbft rust application with namada-inspired arch with diff of not buying into tcp stack (tower~) and using unix sockets. 
There's a clear separation of concerns so code doesn't bleed where not needed.

