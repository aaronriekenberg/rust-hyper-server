# rust-hyper-unixh2c

Similar to [rust-fastcgi](https://github.com/aaronriekenberg/rust-fastcgi), but:

* Using [hyper](https://hyper.rs/) to do cleartext HTTP2 (h2c) over a unix socket
* Running this with Caddy reverse_proxy as:

```
       handle /cgi-bin/* {
                reverse_proxy unix+h2c//path/to/unix/socket
       }
```

This app runs in about 3.5 megabytes of resident memory on a 64-bit Raspberry Pi.
