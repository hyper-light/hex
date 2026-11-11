#!/usr/bin/env python3
"""ROOT-only actual VFIO host-to-guest PCI witness from the selected VMM's API."""

import argparse
import http.client
import json
import re
import socket
import sys
import time
from pathlib import Path

from evidence import InvalidEvidence, finite_positive, require


def pci_tuple(text):
    require(isinstance(text,str) and re.fullmatch(r"[0-9a-f]{4,8}:[0-9a-f]{2}:[0-1][0-9a-f]\.[0-7]",text),
            "noncanonical native PCI address")
    domain,bus,tail=text.split(":")
    device,function=tail.split(".")
    return [int(domain,16),int(bus,16),int(device,16),int(function,16)]


def key(pci):
    domain,bus,device,function=pci
    return (domain<<16)|(bus<<8)|(device<<3)|function


def configured_device(config, host_sbdf):
    devices=config.get("device-passthrough")
    require(isinstance(devices,list) and all(isinstance(device,dict) for device in devices),
            "actual VMM configuration has no typed passthrough list")
    selected=[device for device in devices if device.get("sbdf")==host_sbdf]
    require(len(selected)==1,"selected host PCI is missing or ambiguous in actual VMM configuration")
    device=selected[0]
    identifier=device.get("id")
    # The pinned API's checked_id accepts alphanumeric/underscore segments.
    # Consume the emitted ID; its spelling is not a host-PCI identity codec here.
    require(isinstance(identifier,str) and identifier and identifier.isascii()
            and all(char=="_" or char.isalnum() for char in identifier),
            "actual VMM device ID is not an ASCII API path segment")
    require(sum(item.get("id")==identifier for item in devices)==1,
            "actual VMM configuration repeats the selected device ID")
    return device


def api_json(api, path, deadline, byte_limit):
    connection=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM)
    try:
        left=deadline-time.monotonic()
        require(finite_positive(left),"actual VMM query exhausted caller deadline")
        connection.settimeout(left)
        connection.connect(str(api))
        request=f"GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
        left=deadline-time.monotonic()
        require(finite_positive(left),"actual VMM query exhausted caller deadline")
        connection.settimeout(left)
        connection.sendall(request.encode("ascii"))
        response=http.client.HTTPResponse(connection)
        left=deadline-time.monotonic()
        require(finite_positive(left),"actual VMM query exhausted caller deadline")
        connection.settimeout(left)
        response.begin()
        require(response.status==200,"actual VMM refused physical attachment witness")
        raw=response.read(byte_limit+1)
        require(len(raw)<=byte_limit,"actual attachment response exceeded admitted bound")
        require(time.monotonic()<deadline,"actual VMM query exceeded caller deadline")
        row=json.loads(raw)
        require(isinstance(row,dict),"actual VMM query returned no object")
        return row
    finally:
        connection.close()


def witness(api, host_sbdf, expected_key, timeout, byte_limit):
    host=pci_tuple(host_sbdf)
    require(key(host)==expected_key,"selected native key differs from actual host PCI")
    deadline=time.monotonic()+timeout
    require(finite_positive(deadline),"actual VMM caller deadline overflow")
    device=configured_device(api_json(api,"/vm/config",deadline,byte_limit),host_sbdf)
    identifier=device["id"]
    row=api_json(api,f"/device-passthrough/{identifier}",deadline,byte_limit)
    require(row.get("id")==identifier,"VMM attachment differs from its configured device ID")
    require(row.get("sbdf")==host_sbdf or row.get("host_sbdf")==host_sbdf,
            "VMM witness belongs to another physical host device")
    guest=pci_tuple(row["guest_sbdf"])
    return {"native_key":expected_key,"host_pci":host,"guest_pci":guest,
            "api":str(api),"configured_device":device,"vmm_response":row}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root-authorized",action="store_true")
    parser.add_argument("--api",type=Path,required=True)
    parser.add_argument("--host-sbdf",required=True)
    parser.add_argument("--native-key",type=int,required=True)
    parser.add_argument("--timeout",type=float,required=True)
    parser.add_argument("--byte-limit",type=int,required=True)
    args=parser.parse_args()
    require(args.root_authorized and finite_positive(args.timeout) and args.byte_limit>0,"ROOT finite envelope required")
    print(json.dumps(witness(args.api,args.host_sbdf,args.native_key,args.timeout,args.byte_limit)))


if __name__=="__main__":
    try:
        main()
    except (InvalidEvidence,OSError,ValueError,KeyError,http.client.HTTPException) as error:
        print(f"UNVERIFIED Firecracker physical join: {error}",file=sys.stderr)
        sys.exit(1)
