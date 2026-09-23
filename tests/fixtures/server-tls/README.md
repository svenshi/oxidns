# Local server TLS test identity

These public test fixtures are used only by loopback TCP/DoT integration tests.
The self-signed certificate covers `localhost`; the private key is intentionally
checked in and must never be used for a deployed server. Tests trust this
certificate explicitly and exercise both TLS 1.2 and TLS 1.3.

Generated with:

```sh
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes \
  -keyout key.pem -out cert.pem -days 36500 -subj /CN=localhost \
  -addext subjectAltName=DNS:localhost \
  -addext basicConstraints=critical,CA:FALSE \
  -addext extendedKeyUsage=serverAuth
```
