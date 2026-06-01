use std::future::{ready, Ready};
use std::rc::Rc;
use std::net::IpAddr;
use actix_web::{
    dev::{self, Service, ServiceRequest, ServiceResponse, Transform},
    Error, HttpResponse,
};
use actix_web::body::{EitherBody, MessageBody};
use futures_util::future::LocalBoxFuture;
use ipnetwork::IpNetwork;

pub struct AuthorizationMiddleware {
    trusted_networks: Vec<IpNetwork>,
    api_key: Option<String>,
}

impl AuthorizationMiddleware {
    pub fn new(trusted_networks: Vec<IpNetwork>, api_key: Option<String>) -> Self {
        AuthorizationMiddleware {
            trusted_networks,
            api_key,
        }
    }
}

impl<S, B> Transform<S, ServiceRequest> for AuthorizationMiddleware
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type InitError = ();
    type Transform = AuthorizationMiddlewareService<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(AuthorizationMiddlewareService {
            service: Rc::new(service),
            trusted_networks: self.trusted_networks.clone(),
            api_key: self.api_key.clone(),
        }))
    }
}

pub struct AuthorizationMiddlewareService<S> {
    service: Rc<S>,
    trusted_networks: Vec<IpNetwork>,
    api_key: Option<String>,
}

impl<S, B> Service<ServiceRequest> for AuthorizationMiddlewareService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    dev::forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let ip_opt: Option<IpAddr> = req
            .headers()
            .get("X-Forwarded-For")
            .and_then(|val| val.to_str().ok())
            .and_then(|s| s.split(',').next())
            .and_then(|s| s.trim().parse::<IpAddr>().ok())
            .or_else(|| req.peer_addr().map(|addr| addr.ip()));

        let mut is_trusted = false;
        if let Some(ip) = ip_opt {
            for net in &self.trusted_networks {
                if net.contains(ip) {
                    is_trusted = true;
                    break;
                }
            }
        }

        if !is_trusted {
            let api_key_header = req
                .headers()
                .get("X-Api-Key")
                .and_then(|val| val.to_str().ok())
                .or_else(|| {
                    req.headers()
                        .get("Authorization")
                        .and_then(|val| val.to_str().ok())
                        .and_then(|s| s.strip_prefix("Bearer "))
                });

            if let Some(ref expected_key) = self.api_key {
                if let Some(key) = api_key_header {
                    if key == expected_key {
                        is_trusted = true;
                    }
                }
            }
        }

        if is_trusted {
            let service = self.service.clone();
            Box::pin(async move {
                let res = service.call(req).await?;
                Ok(res.map_body(|_, body| EitherBody::left(body)))
            })
        } else {
            let (request, _pl) = req.into_parts();
            let response = HttpResponse::Forbidden()
                .body("Forbidden: Untrusted IP address or invalid API key")
                .map_into_right_body();
            Box::pin(async move { Ok(ServiceResponse::new(request, response)) })
        }
    }
}
